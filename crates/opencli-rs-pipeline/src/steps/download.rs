use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use opencli_rs_core::{CliError, IPage};
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::step_registry::{StepHandler, StepRegistry};
use crate::template::{render_template_str, TemplateContext};

// ---------------------------------------------------------------------------
// DownloadStep
// ---------------------------------------------------------------------------

/// DownloadStep handles media/article downloads.
/// Supports:
/// - `tool: yt-dlp` — invoke yt-dlp for video downloads
/// - `type: media` — download media files directly
/// - `type: article` — extract article content
pub struct DownloadStep;

/// Download separate DASH video/audio URLs that are already supplied by a
/// browser page, then mux them locally with ffmpeg. Site adapters only select
/// the naturally exposed URLs; this step knows nothing about a specific site.
pub struct DashMuxStep;

#[async_trait]
impl StepHandler for DownloadStep {
    fn name(&self) -> &'static str {
        "download"
    }

    fn is_browser_step(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let obj = params.as_object();

        let tool = obj
            .and_then(|o| o.get("tool"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let ctx = TemplateContext {
            args: args.clone(),
            data: data.clone(),
            item: Value::Null,
            index: 0,
        };

        if tool == "yt-dlp" {
            return execute_ytdlp(obj.unwrap_or(&serde_json::Map::new()), &ctx, data).await;
        }

        let download_type = obj
            .and_then(|o| o.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("media");

        if download_type == "article" {
            return execute_article_download(obj.unwrap_or(&serde_json::Map::new()), &ctx, data)
                .await;
        }

        if download_type == "base64" {
            return execute_base64_save(obj.unwrap_or(&serde_json::Map::new()), &ctx, data).await;
        }

        if download_type == "base64-batch" {
            return execute_base64_batch_download(
                obj.unwrap_or(&serde_json::Map::new()),
                &ctx,
                data,
            )
            .await;
        }

        if download_type == "twitter-media" || download_type == "media-batch" {
            return execute_media_batch_download(
                obj.unwrap_or(&serde_json::Map::new()),
                &ctx,
                data,
            )
            .await;
        }

        // Default: metadata-only download (extract URLs and annotate)

        let url = obj
            .and_then(|o| o.get("url"))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| data.get("url").and_then(|v| v.as_str()).map(String::from));

        let mut result = match data {
            Value::Object(obj) => obj.clone(),
            _ => serde_json::Map::new(),
        };

        result.insert(
            "download_type".to_string(),
            Value::String(download_type.to_string()),
        );
        if let Some(ref u) = url {
            let filename = u
                .rsplit('/')
                .next()
                .unwrap_or("download")
                .split('?')
                .next()
                .unwrap_or("download");
            result.insert("download_url".to_string(), Value::String(u.clone()));
            result.insert(
                "download_path".to_string(),
                Value::String(filename.to_string()),
            );
        }
        result.insert(
            "download_status".to_string(),
            Value::String("pending".to_string()),
        );

        Ok(Value::Object(result))
    }
}

fn render_dash_param(
    params: &serde_json::Map<String, Value>,
    key: &str,
    ctx: &TemplateContext,
) -> Result<String, CliError> {
    let raw = params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::pipeline(format!("dash-mux: missing {key}")))?;
    render_template_str(raw, ctx)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| CliError::pipeline(format!("dash-mux: {key} must render to a string")))
}

async fn download_dash_stream(
    client: &reqwest::Client,
    url: &str,
    path: &std::path::Path,
    referer: Option<&str>,
) -> Result<u64, CliError> {
    let mut request = client.get(url);
    if let Some(referer) = referer.filter(|value| !value.is_empty()) {
        request = request.header(reqwest::header::REFERER, referer);
    }
    let mut response = request.send().await.map_err(|error| {
        CliError::command_execution(format!("dash-mux: media request failed: {error}"))
    })?;
    if !response.status().is_success() {
        return Err(CliError::command_execution(format!(
            "dash-mux: media request returned HTTP {}",
            response.status()
        )));
    }
    let mut file = tokio::fs::File::create(path).await.map_err(|error| {
        CliError::command_execution(format!(
            "dash-mux: cannot create temporary stream file: {error}"
        ))
    })?;
    let mut written = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        CliError::command_execution(format!("dash-mux: media stream failed: {error}"))
    })? {
        file.write_all(&chunk).await.map_err(|error| {
            CliError::command_execution(format!(
                "dash-mux: cannot write temporary stream file: {error}"
            ))
        })?;
        written += chunk.len() as u64;
    }
    file.flush().await.map_err(|error| {
        CliError::command_execution(format!(
            "dash-mux: cannot flush temporary stream file: {error}"
        ))
    })?;
    Ok(written)
}

#[async_trait]
impl StepHandler for DashMuxStep {
    fn name(&self) -> &'static str {
        "dash-mux"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        params: &Value,
        data: &Value,
        args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        let params = params
            .as_object()
            .ok_or_else(|| CliError::pipeline("dash-mux: params must be an object"))?;
        let ctx = TemplateContext {
            args: args.clone(),
            data: data.clone(),
            item: Value::Null,
            index: 0,
        };
        let video_url = render_dash_param(params, "video_url", &ctx)?;
        let audio_url = render_dash_param(params, "audio_url", &ctx)?;
        let output_dir = render_dash_param(params, "output", &ctx)?;
        let title = render_dash_param(params, "title", &ctx)?;
        let filename = render_dash_param(params, "filename", &ctx)?;
        let referer = params
            .get("referer")
            .and_then(Value::as_str)
            .map(|value| render_template_str(value, &ctx))
            .transpose()?
            .and_then(|value| value.as_str().map(str::to_string));
        let user_agent = params
            .get("user_agent")
            .and_then(Value::as_str)
            .map(|value| render_template_str(value, &ctx))
            .transpose()?
            .and_then(|value| value.as_str().map(str::to_string));

        if !is_direct_http_url(&video_url) || !is_direct_http_url(&audio_url) {
            return Err(CliError::argument(
                "dash-mux requires direct HTTP(S) video and audio URLs",
            ));
        }
        if filename.is_empty()
            || filename.contains('/')
            || filename.contains('\\')
            || filename.contains("..")
        {
            return Err(CliError::argument(
                "dash-mux filename must be a simple file name",
            ));
        }

        let output_dir = std::path::PathBuf::from(output_dir);
        std::fs::create_dir_all(&output_dir).map_err(|error| {
            CliError::command_execution(format!(
                "dash-mux: cannot create output directory: {error}"
            ))
        })?;
        let output = output_dir.join(format!("{filename}.mp4"));
        if output.exists() {
            return Err(CliError::command_execution(
                "dash-mux refuses to overwrite an existing output file",
            ));
        }
        let video_part = output_dir.join(format!(".{filename}.video.m4s.part"));
        let audio_part = output_dir.join(format!(".{filename}.audio.m4s.part"));
        let mux_part = output_dir.join(format!(".{filename}.mux.part.mp4"));
        let client = reqwest::Client::builder()
            .user_agent(user_agent.unwrap_or_else(|| {
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36".to_string()
            }))
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|error| {
                CliError::command_execution(format!("dash-mux: client setup failed: {error}"))
            })?;

        let result = async {
            download_dash_stream(&client, &video_url, &video_part, referer.as_deref()).await?;
            download_dash_stream(&client, &audio_url, &audio_part, referer.as_deref()).await?;
            let status = tokio::process::Command::new("ffmpeg")
                .arg("-y")
                .arg("-i")
                .arg(&video_part)
                .arg("-i")
                .arg(&audio_part)
                .arg("-c")
                .arg("copy")
                .arg("-movflags")
                .arg("+faststart")
                .arg(&mux_part)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .map_err(|error| {
                    CliError::command_execution(format!(
                        "dash-mux: ffmpeg could not start: {error}"
                    ))
                })?;
            if !status.success() {
                return Err(CliError::command_execution("dash-mux: ffmpeg mux failed"));
            }
            std::fs::rename(&mux_part, &output).map_err(|error| {
                CliError::command_execution(format!("dash-mux: cannot finalize output: {error}"))
            })?;
            let bytes = std::fs::metadata(&output)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            Ok::<Value, CliError>(serde_json::json!([{
                "title": title,
                "status": "ok",
                "size": format_size(bytes as usize),
                "path": output.display().to_string(),
            }]))
        }
        .await;

        for path in [&video_part, &audio_part, &mux_part] {
            let _ = std::fs::remove_file(path);
        }
        result
    }
}

/// Execute base64 save — decode a data URL and write to disk.
async fn execute_base64_save(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    // Get source: params.source (template) or data[0].response or data.response
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| {
            let item = match data {
                Value::Array(arr) => arr.first(),
                other => Some(other),
            };
            item.and_then(|v| v.get("response"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .ok_or_else(|| CliError::pipeline("download base64: no source data URL found"))?;

    // If the source is an error message, pass it through
    if source.starts_with('[') {
        return Ok(serde_json::json!([{ "response": source, "size": "-" }]));
    }

    // Parse "data:<mime>;base64,<data>"
    let b64_data = if let Some(comma) = source.find(',') {
        &source[comma + 1..]
    } else {
        source.as_str()
    };

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64_data)
        .map_err(|e| CliError::pipeline(format!("base64 decode failed: {}", e)))?;

    // Determine output directory (skip empty strings → default to ~/Downloads)
    let output_dir = params
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{}/Downloads", home)
        });

    // Determine filename
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| {
            let item = match data {
                Value::Array(arr) => arr.first(),
                other => Some(other),
            };
            item.and_then(|v| v.get("filename"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| {
            format!(
                "image_{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            )
        });

    let _ = std::fs::create_dir_all(&output_dir);
    let file_path = format!("{}/{}", output_dir, filename);
    std::fs::write(&file_path, &bytes)
        .map_err(|e| CliError::pipeline(format!("failed to write image: {}", e)))?;

    let size = format_size(bytes.len());
    info!(path = %file_path, size = %size, "Saved base64 image");

    Ok(serde_json::json!([{ "response": file_path, "size": size }]))
}

/// Execute article download — save markdown content to file with image localization
async fn execute_article_download(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| data.get("title").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "article".to_string());

    let output_dir = params
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_else(|| "./articles".to_string());

    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| {
            data.get("filename")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "article.md".to_string());

    let mut content = params
        .get("content")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| {
            data.get("content")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_default();

    let url = data.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let author = data.get("author").and_then(|v| v.as_str()).unwrap_or("-");
    let download_path = if filename != "article.md" {
        filename.clone()
    } else {
        url.rsplit('/').next().unwrap_or("article.pdf").to_string()
    };

    if content.is_empty() {
        return Ok(serde_json::json!({
            "title": title,
            "author": author,
            "status": "failed",
            "size": "No content to save",
            "download_type": "article",
            "download_url": url,
            "download_path": download_path,
        }));
    }

    // Create article directory (output/safe_title/)
    let safe_title: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(80)
        .collect();
    let article_dir = format!("{}/{}", output_dir, safe_title);
    let _ = std::fs::create_dir_all(&article_dir);

    // Download images if present in data
    let image_urls: Vec<String> = data
        .get("imageUrls")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if !image_urls.is_empty() {
        let images_dir = format!("{}/images", article_dir);
        let _ = std::fs::create_dir_all(&images_dir);

        let referer = data.get("referer").and_then(|v| v.as_str()).unwrap_or("");
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        let mut seen = std::collections::HashSet::new();
        let mut img_index = 0;

        for raw_url in &image_urls {
            if seen.contains(raw_url.as_str()) {
                continue;
            }
            seen.insert(raw_url.as_str());
            img_index += 1;

            let mut img_url = raw_url.clone();
            if img_url.starts_with("//") {
                img_url = format!("https:{}", img_url);
            }

            // Detect extension
            let ext = if let Some(m) = img_url.find("wx_fmt=") {
                img_url[m + 7..]
                    .split(&['&', '?', ' '][..])
                    .next()
                    .unwrap_or("png")
                    .to_string()
            } else {
                img_url
                    .rsplit('.')
                    .next()
                    .and_then(|e| e.split(&['?', '#', '&'][..]).next())
                    .filter(|e| e.len() <= 5 && e.chars().all(|c| c.is_alphanumeric()))
                    .unwrap_or("jpg")
                    .to_string()
            };

            let img_filename = format!("img_{:03}.{}", img_index, ext);
            let img_path = format!("{}/{}", images_dir, img_filename);
            let local_path = format!("images/{}", img_filename);

            let mut req = client.get(&img_url);
            if !referer.is_empty() {
                req = req.header("Referer", referer);
            }

            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(bytes) = resp.bytes().await {
                        if std::fs::write(&img_path, &bytes).is_ok() {
                            debug!(img = %img_filename, size = bytes.len(), "Image downloaded");
                            // Replace remote URL with local path in markdown
                            content = content.replace(raw_url.as_str(), &local_path);
                        }
                    }
                }
                _ => {
                    debug!(url = %img_url, "Image download failed, keeping remote URL");
                }
            }
        }
        info!(count = img_index, "Images downloaded");
    }

    // Write markdown file
    let file_path = format!("{}/{}", article_dir, filename);
    match std::fs::write(&file_path, &content) {
        Ok(_) => {
            let size = content.len();
            let size_str = if size > 1_000_000 {
                format!("{:.1} MB", size as f64 / 1e6)
            } else if size > 1000 {
                format!("{:.1} KB", size as f64 / 1e3)
            } else {
                format!("{} bytes", size)
            };

            info!(title = %title, path = %file_path, size = %size_str, "Article saved");

            Ok(serde_json::json!({
                "title": title,
                "author": author,
                "status": "ok",
                "size": size_str,
                "path": file_path,
                "images": image_urls.len(),
                "download_url": url,
                "download_path": download_path,
                "download_type": "article",
            }))
        }
        Err(e) => Ok(serde_json::json!({
            "title": title,
            "author": author,
            "status": "failed",
            "size": format!("Write error: {}", e),
            "download_type": "article",
            "download_url": url,
            "download_path": download_path,
        })),
    }
}

/// Execute batch base64 download - decode multiple base64 images and write to disk
async fn execute_base64_batch_download(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    let output_dir = params
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{}/Downloads", home)
        });

    let items = data
        .get("images")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if items.is_empty() {
        if data.is_array() {
            return Ok(data.clone());
        }
        // Debug: log what data we actually got
        let debug_msg = format!(
            "No images to download. Data structure: {}",
            serde_json::to_string(data).unwrap_or_else(|_| "unserializable".to_string())
        );
        info!("{}", debug_msg);
        return Ok(serde_json::json!([{ "filename": "-", "status": "failed", "size": debug_msg }]));
    }

    let _ = std::fs::create_dir_all(&output_dir);

    let mut results = Vec::new();

    for item in items.iter() {
        let filename = item
            .get("filename")
            .and_then(|v| v.as_str())
            .unwrap_or("image.png");
        let response = item.get("response").and_then(|v| v.as_str()).unwrap_or("");

        if response.is_empty() {
            results.push(serde_json::json!({
                "filename": filename,
                "status": "failed",
                "size": "No base64 data"
            }));
            continue;
        }

        // Parse "data:<mime>;base64,<data>"
        let b64_data = if let Some(comma) = response.find(',') {
            &response[comma + 1..]
        } else {
            response
        };

        match base64::engine::general_purpose::STANDARD.decode(b64_data) {
            Ok(bytes) => {
                let file_path = format!("{}/{}", output_dir, filename);
                match std::fs::write(&file_path, &bytes) {
                    Ok(_) => {
                        let size = format_size(bytes.len());
                        info!(filename = %filename, path = %file_path, size = %size, "Saved base64 image");
                        results.push(serde_json::json!({
                            "filename": filename,
                            "status": "ok",
                            "size": size,
                            "path": file_path
                        }));
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "filename": filename,
                            "status": "failed",
                            "size": format!("Write error: {}", e)
                        }));
                    }
                }
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "filename": filename,
                    "status": "failed",
                    "size": format!("Decode error: {}", e)
                }));
            }
        }
    }

    // Preserve original data structure while adding download results
    let mut result = match data {
        Value::Object(obj) => obj.clone(),
        _ => serde_json::Map::new(),
    };

    if results.is_empty() {
        result.insert(
            "download_results".to_string(),
            serde_json::json!([{ "filename": "-", "status": "no images", "size": "-" }]),
        );
    } else {
        let count = results.len();
        result.insert("download_results".to_string(), Value::Array(results));
        info!(count = count, dir = %output_dir, "Base64 batch download complete");
    }

    Ok(Value::Object(result))
}

/// Execute batch media download (images + videos from a list)
async fn execute_media_batch_download(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    let output_dir = params
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_else(|| "./downloads".to_string());

    let prefix = params
        .get("username")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_else(|| "media".to_string());

    let save_metadata = params
        .get("saveMetadata")
        .or_else(|| params.get("save_metadata"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let metadata_filename = params
        .get("metadataFilename")
        .or_else(|| params.get("metadata_filename"))
        .and_then(Value::as_str)
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_else(|| "note.md".to_string());

    let items = data
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if items.is_empty() && !save_metadata {
        // Data might already be an upstream error/status array — surface it as-is.
        if data.is_array() {
            return Ok(data.clone());
        }
        // No media resolved: fail the step instead of silently succeeding with
        // zero downloads, so daemon jobs land in `failed` (with retry/backoff)
        // rather than `done` with an empty result — see docs/debug.md.
        return Err(CliError::pipeline(
            "download: no media items to download (upstream step produced an empty items list)",
        ));
    }

    let _ = std::fs::create_dir_all(&output_dir);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let referer = params.get("referer").and_then(Value::as_str).map(|s| {
        render_template_str(s, ctx)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| s.to_string())
    });

    let mut results = Vec::new();

    for (i, item) in items.iter().enumerate() {
        let media_type = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");

        if url.is_empty() {
            continue;
        }

        let idx = i + 1;

        if media_type == "image" || media_type == "video" {
            if !is_direct_http_url(url) {
                results.push(serde_json::json!({
                    "index": idx,
                    "type": media_type,
                    "status": unsupported_media_status(url, item.get("reason").and_then(Value::as_str)),
                    "size": "-"
                }));
                continue;
            }

            match download_direct_media(
                &client,
                url,
                &output_dir,
                &prefix,
                idx,
                media_type,
                referer.as_deref(),
            )
            .await
            {
                Ok((filename, size)) => results.push(serde_json::json!({
                    "index": idx, "type": media_type, "status": "ok", "size": size, "path": filename
                })),
                Err(error) => {
                    debug!(%error, media_type, "Direct media download failed");
                    results.push(serde_json::json!({
                        "index": idx, "type": media_type, "status": "failed", "size": "-"
                    }));
                }
            }
        } else if media_type == "video-tweet" {
            // Use yt-dlp for tweet videos
            let filename = format!("{}_{:03}.mp4", prefix, idx);
            let filepath = format!("{}/{}", output_dir, filename);

            let status = tokio::process::Command::new("yt-dlp")
                .args([
                    "-f",
                    "best[ext=mp4]/best",
                    "--merge-output-format",
                    "mp4",
                    "-o",
                    &filepath,
                    url,
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;

            match status {
                Ok(s) if s.success() => {
                    let size = std::fs::metadata(&filepath)
                        .map(|m| format_size(m.len() as usize))
                        .unwrap_or("-".to_string());
                    results.push(serde_json::json!({
                        "index": idx, "type": "video", "status": "ok", "size": size
                    }));
                }
                _ => {
                    results.push(serde_json::json!({
                        "index": idx, "type": "video", "status": "failed (yt-dlp)", "size": "-"
                    }));
                }
            }
        }
    }

    if save_metadata {
        match write_media_metadata(&output_dir, &metadata_filename, data, &results) {
            Ok(filename) => results.push(serde_json::json!({
                "index": 0, "type": "metadata", "status": "ok", "size": "saved", "path": filename
            })),
            Err(error) => results.push(serde_json::json!({
                "index": 0, "type": "metadata", "status": format!("failed: {}", error), "size": "-"
            })),
        }
    }

    if results.is_empty() {
        return Ok(
            serde_json::json!([{ "index": 0, "type": "-", "status": "no media", "size": "-" }]),
        );
    }

    info!(count = results.len(), dir = %output_dir, "Media batch download complete");
    Ok(Value::Array(results))
}

fn is_direct_http_url(url: &str) -> bool {
    matches!(reqwest::Url::parse(url), Ok(parsed) if matches!(parsed.scheme(), "http" | "https"))
}

fn unsupported_media_status(url: &str, reason: Option<&str>) -> String {
    if let Some(reason) = reason.filter(|reason| !reason.trim().is_empty()) {
        return format!("unsupported: {}", reason);
    }
    if url.starts_with("blob:") {
        "unsupported: browser blob/MSE stream".to_string()
    } else if url.contains(".m3u8") {
        "unsupported: HLS stream".to_string()
    } else {
        "unsupported: non-HTTP media URL".to_string()
    }
}

async fn download_direct_media(
    client: &reqwest::Client,
    url: &str,
    output_dir: &str,
    prefix: &str,
    index: usize,
    media_type: &str,
    referer: Option<&str>,
) -> Result<(String, String), String> {
    let mut request = client.get(url);
    if let Some(referer) = referer {
        request = request.header(reqwest::header::REFERER, referer);
    }
    let mut response = request.send().await.map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let extension = media_extension(
        url,
        response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        media_type,
    );
    let filename = format!("{}_{:03}.{}", prefix, index, extension);
    let path = std::path::Path::new(output_dir).join(&filename);
    let partial_path = path.with_extension(format!("{}.part", extension));
    let mut file = tokio::fs::File::create(&partial_path)
        .await
        .map_err(|error| error.to_string())?;
    let mut byte_count = 0usize;

    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        file.write_all(&chunk)
            .await
            .map_err(|error| error.to_string())?;
        byte_count += chunk.len();
    }
    file.flush().await.map_err(|error| error.to_string())?;
    drop(file);
    tokio::fs::rename(&partial_path, &path)
        .await
        .map_err(|error| error.to_string())?;

    Ok((filename, format_size(byte_count)))
}

fn media_extension(url: &str, content_type: Option<&str>, media_type: &str) -> &'static str {
    let content_type = content_type.unwrap_or("").to_ascii_lowercase();
    if content_type.contains("webp") || url.to_ascii_lowercase().contains("webp") {
        "webp"
    } else if content_type.contains("png") || url.to_ascii_lowercase().contains("png") {
        "png"
    } else if content_type.contains("avif") || url.to_ascii_lowercase().contains("avif") {
        "avif"
    } else if content_type.contains("gif") || url.to_ascii_lowercase().contains("gif") {
        "gif"
    } else if content_type.contains("jpeg")
        || content_type.contains("jpg")
        || url.to_ascii_lowercase().contains("jpg")
        || url.to_ascii_lowercase().contains("jpeg")
    {
        "jpg"
    } else if content_type.contains("webm") {
        "webm"
    } else if media_type == "video" {
        "mp4"
    } else {
        "jpg"
    }
}

fn write_media_metadata(
    output_dir: &str,
    requested_filename: &str,
    data: &Value,
    results: &[Value],
) -> Result<String, String> {
    let filename = std::path::Path::new(requested_filename)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("note.md");
    let path = std::path::Path::new(output_dir).join(filename);
    let title = data
        .get("title")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("untitled");
    let author = data
        .get("author")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("unknown");
    let content = data
        .get("content")
        .or_else(|| data.get("desc"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let source = data
        .get("sourceUrl")
        .or_else(|| data.get("source_url"))
        .and_then(Value::as_str)
        .map(redact_source_url)
        .unwrap_or_default();
    let note_type = data
        .get("noteType")
        .or_else(|| data.get("note_type"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut markdown = format!("# {}\n\n- 作者：{}\n", title, author);
    if !note_type.is_empty() {
        markdown.push_str(&format!("- 类型：{}\n", note_type));
    }
    if !source.is_empty() {
        markdown.push_str(&format!("- 来源：{}\n", source));
    }
    markdown.push_str("\n## 正文\n\n");
    markdown.push_str(content.trim());
    markdown.push_str("\n\n## 媒体\n\n");
    for result in results {
        let media_type = result
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("media");
        let status = result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let path = result.get("path").and_then(Value::as_str);
        if let Some(path) = path {
            markdown.push_str(&format!("- {}：`{}`（{}）\n", media_type, path, status));
        } else {
            markdown.push_str(&format!("- {}：{}\n", media_type, status));
        }
    }
    std::fs::write(&path, markdown).map_err(|error| error.to_string())?;
    Ok(filename.to_string())
}

fn redact_source_url(source: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(source) else {
        return source.to_string();
    };
    let safe_query: Vec<(String, String)> = parsed
        .query_pairs()
        .filter(|(key, _)| !matches!(key.as_ref(), "xsec_token" | "token" | "signature" | "sign"))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    parsed.query_pairs_mut().clear().extend_pairs(safe_query);
    parsed.to_string()
}

fn format_size(bytes: usize) -> String {
    if bytes > 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else if bytes > 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else if bytes > 1000 {
        format!("{:.1} KB", bytes as f64 / 1e3)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Execute yt-dlp download
async fn execute_ytdlp(
    params: &serde_json::Map<String, Value>,
    ctx: &TemplateContext,
    data: &Value,
) -> Result<Value, CliError> {
    // Check if yt-dlp is installed
    let ytdlp_ok = tokio::process::Command::new(if cfg!(target_os = "windows") {
        "where"
    } else {
        "which"
    })
    .arg("yt-dlp")
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .status()
    .await
    .map(|s| s.success())
    .unwrap_or(false);

    if !ytdlp_ok {
        return Ok(serde_json::json!([{
            "status": "failed",
            "size": "yt-dlp not installed. Run: pip install yt-dlp"
        }]));
    }

    // Render template params
    let url = params
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| data.get("url").and_then(|v| v.as_str()).map(String::from))
        .ok_or_else(|| CliError::pipeline("download: missing url"))?;

    let title = params
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .or_else(|| data.get("title").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| "video".to_string());

    let output_dir = params
        .get("output")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_else(|| "./downloads".to_string());

    let quality = params
        .get("quality")
        .and_then(|v| v.as_str())
        .map(|s| {
            render_template_str(s, ctx)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_else(|| "best".to_string());

    let user_agent = params
        .get("user_agent")
        .and_then(|value| value.as_str())
        .map(|value| {
            render_template_str(value, ctx)
                .ok()
                .and_then(|rendered| rendered.as_str().map(String::from))
                .unwrap_or_else(|| value.to_string())
        })
        .or_else(|| {
            data.get("user_agent")
                .and_then(|value| value.as_str())
                .map(String::from)
        });
    let referer = params
        .get("referer")
        .and_then(|value| value.as_str())
        .map(|value| {
            render_template_str(value, ctx)
                .ok()
                .and_then(|rendered| rendered.as_str().map(String::from))
                .unwrap_or_else(|| value.to_string())
        })
        .or_else(|| {
            data.get("referer")
                .and_then(|value| value.as_str())
                .map(String::from)
        });

    let dry_run = params
        .get("dry_run")
        .and_then(|value| match value {
            Value::Bool(value) => Some(*value),
            Value::String(raw) => render_template_str(raw, ctx).ok().and_then(|rendered| {
                rendered.as_bool().or_else(|| {
                    rendered
                        .as_str()
                        .map(|value| value.eq_ignore_ascii_case("true"))
                })
            }),
            _ => None,
        })
        .unwrap_or(false);

    // Extract cookies from data (set by evaluate step from document.cookie)
    let cookies_str = data.get("cookies").and_then(|v| v.as_str()).unwrap_or("");

    // Sanitize title for filename
    let safe_title: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .chars()
        .take(100)
        .collect();

    // Build yt-dlp format string
    let format = match quality.as_str() {
        "1080p" => "bestvideo[height<=1080][ext=mp4]+bestaudio[ext=m4a]/best[height<=1080]",
        "720p" => "bestvideo[height<=720][ext=mp4]+bestaudio[ext=m4a]/best[height<=720]",
        "480p" => "bestvideo[height<=480][ext=mp4]+bestaudio[ext=m4a]/best[height<=480]",
        _ => "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
    };

    let output_path = format!("{}/{}.mp4", output_dir, safe_title);

    // yt-dlp needs the browser session on sites that reject anonymous media
    // metadata requests. Keep a short-lived Netscape file in the system temp
    // directory, never beside user downloads, and remove it on every path.
    let cookie_file = if !cookies_str.is_empty() {
        let cookie_path = std::env::temp_dir().join(format!(
            "opencli-ytdlp-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0),
        ));
        let domain = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .and_then(|s| s.split('/').next())
            .map(|host| {
                let parts: Vec<&str> = host.split('.').collect();
                if parts.len() >= 2 {
                    format!(".{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
                } else {
                    format!(".{}", host)
                }
            })
            .unwrap_or_else(|| ".example.com".to_string());

        let mut netscape = String::from("# Netscape HTTP Cookie File\n");
        for cookie in cookies_str.split(';') {
            let cookie = cookie.trim();
            if let Some((name, value)) = cookie.split_once('=') {
                netscape.push_str(&format!(
                    "{}\tTRUE\t/\tFALSE\t0\t{}\t{}\n",
                    domain,
                    name.trim(),
                    value.trim()
                ));
            }
        }
        std::fs::write(&cookie_path, netscape).map_err(|error| {
            CliError::command_execution(format!(
                "Failed to prepare temporary yt-dlp session: {error}"
            ))
        })?;
        Some(cookie_path)
    } else {
        None
    };

    if dry_run {
        let mut command = tokio::process::Command::new("yt-dlp");
        command
            .arg("--simulate")
            .arg("--no-playlist")
            .arg("-f")
            .arg(&format)
            .arg(&url);
        if let Some(user_agent) = &user_agent {
            command.arg("--user-agent").arg(user_agent);
        }
        if let Some(referer) = &referer {
            command.arg("--referer").arg(referer);
        }
        if let Some(path) = &cookie_file {
            command.arg("--cookies").arg(path);
        }
        let result = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
        if let Some(path) = &cookie_file {
            let _ = std::fs::remove_file(path);
        }
        let status = result
            .map_err(|e| CliError::command_execution(format!("Failed to run yt-dlp: {}", e)))?;
        return Ok(serde_json::json!([{
            "title": title,
            "status": if status.success() { "dry-run ok" } else { "dry-run failed" },
            "size": "-",
            "path": "",
        }]));
    }

    // Only materialize an output directory for a real download. A dry run
    // should not leave files, cookie material, or empty directories behind.
    let _ = std::fs::create_dir_all(&output_dir);

    info!(url = %url, output = %output_path, "Downloading with yt-dlp");

    let mut cmd = tokio::process::Command::new("yt-dlp");
    cmd.arg("-f")
        .arg(format)
        .arg("--merge-output-format")
        .arg("mp4")
        .arg("--embed-thumbnail")
        .arg("-o")
        .arg(&output_path);

    if let Some(user_agent) = &user_agent {
        cmd.arg("--user-agent").arg(user_agent);
    }
    if let Some(referer) = &referer {
        cmd.arg("--referer").arg(referer);
    }

    if let Some(cf) = &cookie_file {
        cmd.arg("--cookies").arg(cf);
    }

    cmd.arg(&url);

    let command_result = cmd
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .await;

    // Clean up cookie file even if yt-dlp could not start.
    if let Some(cf) = &cookie_file {
        let _ = std::fs::remove_file(cf);
    }

    let status = command_result
        .map_err(|e| CliError::command_execution(format!("Failed to run yt-dlp: {}", e)))?;

    let file_size = std::fs::metadata(&output_path)
        .map(|m| {
            let bytes = m.len();
            if bytes > 1_000_000_000 {
                format!("{:.1} GB", bytes as f64 / 1e9)
            } else if bytes > 1_000_000 {
                format!("{:.1} MB", bytes as f64 / 1e6)
            } else {
                format!("{:.0} KB", bytes as f64 / 1e3)
            }
        })
        .unwrap_or_else(|_| "-".to_string());

    let result_status = if status.success() { "ok" } else { "failed" };

    debug!(status = %result_status, size = %file_size, "yt-dlp download complete");

    Ok(serde_json::json!([{
        "title": title,
        "status": result_status,
        "size": file_size,
        "path": output_path,
    }]))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register_download_steps(registry: &mut StepRegistry) {
    registry.register(Arc::new(DownloadStep));
    registry.register(Arc::new(DashMuxStep));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_args() -> HashMap<String, Value> {
        HashMap::new()
    }

    #[tokio::test]
    async fn test_download_step_registers() {
        let mut registry = StepRegistry::new();
        register_download_steps(&mut registry);
        assert!(registry.get("download").is_some());
    }

    #[test]
    fn test_download_is_browser_step() {
        assert!(DownloadStep.is_browser_step());
    }

    #[tokio::test]
    async fn test_download_with_url_in_params() {
        let step = DownloadStep;
        let params = json!({"type": "media", "url": "https://example.com/video.mp4"});
        let result = step
            .execute(None, &params, &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_url"], "https://example.com/video.mp4");
        assert_eq!(result["download_path"], "video.mp4");
        assert_eq!(result["download_type"], "media");
        assert_eq!(result["download_status"], "pending");
    }

    #[tokio::test]
    async fn test_download_with_url_in_data() {
        let step = DownloadStep;
        let params = json!({"type": "article"});
        let data = json!({"url": "https://example.com/article.pdf", "title": "Test"});
        let result = step
            .execute(None, &params, &data, &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_url"], "https://example.com/article.pdf");
        assert_eq!(result["download_path"], "article.pdf");
        assert_eq!(result["download_type"], "article");
        assert_eq!(result["title"], "Test");
    }

    #[tokio::test]
    async fn test_download_no_url() {
        let step = DownloadStep;
        let result = step
            .execute(None, &json!(null), &json!(null), &empty_args())
            .await
            .unwrap();
        assert_eq!(result["download_status"], "pending");
        assert!(result.get("download_url").is_none());
    }

    #[test]
    fn media_batch_rejects_browser_blob_urls_without_requesting_them() {
        assert!(!is_direct_http_url(
            "blob:https://www.xiaohongshu.com/stream-id"
        ));
        assert_eq!(
            unsupported_media_status("blob:https://www.xiaohongshu.com/stream-id", None),
            "unsupported: browser blob/MSE stream"
        );
    }

    #[test]
    fn media_batch_uses_content_type_or_url_for_extension() {
        assert_eq!(
            media_extension("https://cdn.example/image", Some("image/webp"), "image"),
            "webp"
        );
        assert_eq!(
            media_extension("https://cdn.example/video.mp4", None, "video"),
            "mp4"
        );
    }

    #[test]
    fn metadata_redacts_xsec_token_and_lists_media() {
        let unique = format!(
            "opencli-download-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let output_dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&output_dir).unwrap();
        let data = json!({
            "title": "Example note",
            "author": "author",
            "content": "正文内容",
            "noteType": "normal",
            "sourceUrl": "https://www.xiaohongshu.com/explore/abc?xsec_token=secret&xsec_source=pc_feed"
        });
        let results = vec![json!({
            "type": "image", "status": "ok", "path": "abc_001.webp"
        })];

        let filename =
            write_media_metadata(output_dir.to_str().unwrap(), "note.md", &data, &results).unwrap();
        let markdown = std::fs::read_to_string(output_dir.join(&filename)).unwrap();
        assert!(markdown.contains("正文内容"));
        assert!(markdown.contains("`abc_001.webp`"));
        assert!(markdown.contains("xsec_source=pc_feed"));
        assert!(!markdown.contains("xsec_token"));
        std::fs::remove_dir_all(output_dir).unwrap();
    }
}

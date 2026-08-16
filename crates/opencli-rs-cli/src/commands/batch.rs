//! `opencli batch` — run an item adapter once per row from a list adapter or JSON file.

use clap::ArgMatches;
use opencli_rs_core::{CliCommand, CliError, Registry};
use opencli_rs_engine::execute_command;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub async fn run_batch(matches: &ArgMatches, registry: &Registry) -> Result<(), CliError> {
    let site = matches
        .get_one::<String>("site")
        .ok_or_else(|| CliError::argument("batch requires <site>"))?;
    let list_cmd = matches.get_one::<String>("list_cmd").map(String::as_str);
    let each_cmd = matches
        .get_one::<String>("each")
        .ok_or_else(|| CliError::argument("batch requires --each <command>"))?;
    let id_field = matches
        .get_one::<String>("id")
        .ok_or_else(|| CliError::argument("batch requires --id <field>"))?;
    let out_dir = PathBuf::from(
        matches
            .get_one::<String>("out")
            .ok_or_else(|| CliError::argument("batch requires --out <dir>"))?,
    );
    let limit = matches
        .get_one::<String>("limit")
        .and_then(|s| s.parse::<i64>().ok());
    let sleep_s = matches
        .get_one::<String>("sleep")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.4);
    let resume = matches.get_flag("resume");
    let list_all = matches.get_flag("all");
    let incremental = matches.get_flag("incremental");
    let items_file = matches.get_one::<String>("items").map(PathBuf::from);
    let name_field = matches
        .get_one::<String>("name-field")
        .map(String::as_str)
        .unwrap_or("name");

    fs::create_dir_all(&out_dir)?;
    let progress_path = out_dir.join("progress.json");
    let mut progress = load_progress(&progress_path);

    let rows = if let Some(path) = items_file {
        load_items(&path)?
    } else {
        let list_name = list_cmd.ok_or_else(|| {
            CliError::argument("batch requires <list-command> or --items FILE")
        })?;
        let cmd = registry.get(site, list_name).ok_or_else(|| {
            CliError::argument(format!("unknown list adapter: {site} {list_name}"))
        })?;
        let mut kwargs = defaults_from(cmd);
        if list_all {
            kwargs.insert("all".into(), Value::Bool(true));
        }
        eprintln!("[batch] list {site} {list_name}");
        let data = execute_command(cmd, kwargs).await?;
        rows_from(data)
    };

    let each = registry.get(site, each_cmd).ok_or_else(|| {
        CliError::argument(format!("unknown item adapter: {site} {each_cmd}"))
    })?;

    let following_path = out_dir.join("following.json");
    write_json(&following_path, &Value::Array(rows.clone()))?;

    let mut ok_n: u64 = 0;
    let mut skip_n: u64 = 0;
    let mut fail_n: u64 = 0;
    let mut posts_n: u64 = 0;
    let started = Instant::now();

    for (i, person) in rows.iter().enumerate() {
        let uid = field_str(person, id_field);
        if uid.is_empty() {
            eprintln!(
                "[batch] skip row {}: missing id field `{id_field}`",
                i + 1
            );
            continue;
        }
        let name = field_str(person, name_field);
        let folder = out_dir
            .join("users")
            .join(safe_name(&format!("{uid}_{name}"), &uid));
        let data_path = folder.join("data.json");
        let key = format!("{site}:{uid}");

        if resume && progress_done(&progress, &key) && data_path.exists() {
            skip_n += 1;
            eprintln!("[batch {}/{}] SKIP {name} ({uid})", i + 1, rows.len());
            continue;
        }

        eprintln!("[batch {}/{}] FETCH {name} ({uid})", i + 1, rows.len());
        let mut kwargs = defaults_from(each);
        // Prefer a named arg matching --id (user / uid / id), else first positional.
        if each.args.iter().any(|a| a.name == *id_field) {
            kwargs.insert(id_field.clone(), Value::String(uid.clone()));
        } else if let Some(pos) = each.args.iter().find(|a| a.positional) {
            kwargs.insert(pos.name.clone(), Value::String(uid.clone()));
        } else {
            kwargs.insert(id_field.clone(), Value::String(uid.clone()));
        }
        if let Some(n) = limit {
            kwargs.insert("limit".into(), json!(n));
        }
        if incremental {
            kwargs.insert("incremental".into(), Value::Bool(true));
        }

        match execute_command(each, kwargs).await {
            Ok(data) => {
                let rows_out = match data {
                    Value::Array(a) => a,
                    other => vec![other],
                };
                fs::create_dir_all(&folder)?;
                write_json(&data_path, &Value::Array(rows_out.clone()))?;
                write_json(
                    &folder.join("meta.json"),
                    &json!({
                        "platform": site,
                        "id": uid,
                        "name": name,
                        "profile": person,
                        "post_count": rows_out.len(),
                        "fetched_at": chrono_now(),
                        "limit": limit,
                    }),
                )?;
                mark_done(&mut progress, &key, &name, rows_out.len(), &data_path, &out_dir);
                save_progress(&progress_path, &progress)?;
                ok_n += 1;
                posts_n += rows_out.len() as u64;
                eprintln!("  -> {} items", rows_out.len());
            }
            Err(err) => {
                let err = err.classify();
                if err.is_soft_empty() {
                    fs::create_dir_all(&folder)?;
                    write_json(&data_path, &json!([]))?;
                    write_json(
                        &folder.join("meta.json"),
                        &json!({
                            "platform": site,
                            "id": uid,
                            "name": name,
                            "post_count": 0,
                            "warning": err.to_string(),
                            "fetched_at": chrono_now(),
                        }),
                    )?;
                    mark_done(&mut progress, &key, &name, 0, &data_path, &out_dir);
                    save_progress(&progress_path, &progress)?;
                    ok_n += 1;
                    eprintln!("  -> empty ({})", err.code());
                } else {
                    mark_fail(&mut progress, &key, &name, &err.to_string());
                    save_progress(&progress_path, &progress)?;
                    fail_n += 1;
                    eprintln!("  FAIL {}: {}", name, err);
                }
            }
        }

        if sleep_s > 0.0 {
            tokio::time::sleep(Duration::from_secs_f64(sleep_s)).await;
        }
    }

    let stats = json!({
        "following": rows.len(),
        "ok": ok_n,
        "skip": skip_n,
        "fail": fail_n,
        "posts": posts_n,
    });
    write_json(
        &out_dir.join("summary.json"),
        &json!({
            "site": site,
            "each": each_cmd,
            "elapsed_s": started.elapsed().as_secs_f64(),
            "stats": stats,
        }),
    )?;
    eprintln!(
        "[batch] done ok={ok_n} fail={fail_n} skip={skip_n} posts={posts_n}"
    );
    Ok(())
}

fn defaults_from(cmd: &CliCommand) -> HashMap<String, Value> {
    let mut kwargs = HashMap::new();
    for arg in &cmd.args {
        if let Some(default) = &arg.default {
            kwargs.insert(arg.name.clone(), default.clone());
        }
    }
    kwargs
}

fn rows_from(data: Value) -> Vec<Value> {
    match data {
        Value::Array(a) => a,
        Value::Object(map) => {
            if let Some(Value::Array(a)) = map.get("rows") {
                return a.clone();
            }
            vec![Value::Object(map)]
        }
        other => vec![other],
    }
}

fn field_str(row: &Value, field: &str) -> String {
    row.get(field)
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => v.as_str().map(String::from),
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn safe_name(s: &str, fallback: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | ' ' => '_',
            _ => c,
        })
        .collect();
    let collapsed = cleaned.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_");
    let out = collapsed.trim_matches('.').chars().take(80).collect::<String>();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

fn load_items(path: &Path) -> Result<Vec<Value>, CliError> {
    let text = fs::read_to_string(path)?;
    let val: Value = serde_json::from_str(&text)?;
    Ok(rows_from(val))
}

fn load_progress(path: &Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({"done": {}, "failed": {}}))
}

fn progress_done(progress: &Value, key: &str) -> bool {
    progress
        .get("done")
        .and_then(|d| d.get(key))
        .is_some()
}

fn mark_done(progress: &mut Value, key: &str, name: &str, count: usize, path: &Path, root: &Path) {
    let rel = path.strip_prefix(root).unwrap_or(path);
    progress["done"][key] = json!({
        "name": name,
        "count": count,
        "path": rel.to_string_lossy(),
    });
    if let Some(obj) = progress.get_mut("failed").and_then(|v| v.as_object_mut()) {
        obj.remove(key);
    }
}

fn mark_fail(progress: &mut Value, key: &str, name: &str, error: &str) {
    progress["failed"][key] = json!({ "name": name, "error": error });
}

fn save_progress(path: &Path, progress: &Value) -> Result<(), CliError> {
    write_json(path, progress)
}

fn write_json(path: &Path, value: &Value) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)? + "\n")?;
    Ok(())
}

fn chrono_now() -> String {
    // Avoid extra chrono dep in this crate: RFC3339-ish UTC from system time.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

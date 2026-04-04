/// Adapter 语料库测试
///
/// 这个文件做两件事：
///
/// 1. **静态校验**：扫描 adapters/ 目录下所有 yaml 文件，验证：
///    - 都能被 parse_yaml_adapter 正确解析
///    - 关键字段满足语义约束（domain、description、pipeline 结构等）
///
/// 2. **Pipeline 集成测试**：针对具体 adapter，注入 mock 数据，
///    执行其非浏览器 transform 步骤，验证输出结构与 columns 定义一致。
///    （浏览器步骤/fetch 步骤不在这里测试，那些需要真实环境）
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use opencli_rs_core::{CliError, IPage};
use opencli_rs_discovery::yaml_parser::parse_yaml_adapter;
use opencli_rs_pipeline::steps::register_transform_steps;
use opencli_rs_pipeline::{execute_pipeline, StepHandler, StepRegistry};
use serde_json::Value;

// ─────────────────────────────────────────────────────────────────────────────
// 工具函数
// ─────────────────────────────────────────────────────────────────────────────

fn adapters_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/opencli-rs-discovery
    // adapters/ 在 workspace 根目录
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapters")
        .canonicalize()
        .expect("adapters/ directory not found — run tests from workspace root")
}

fn collect_yaml_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    if !dir.exists() {
        return result;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return result;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            result.extend(collect_yaml_files(&path));
        } else if path
            .extension()
            .map_or(false, |e| e == "yaml" || e == "yml")
        {
            result.push(path);
        }
    }
    result
}

/// 浏览器/网络步骤名，这些步骤在测试中跳过
const BROWSER_STEPS: &[&str] = &[
    "evaluate",
    "navigate",
    "click",
    "type",
    "wait",
    "press",
    "screenshot",
    "snapshot",
    "intercept",
    "tap",
    "fetch",
    "download",
];

fn is_browser_step(step: &Value) -> bool {
    step.as_object()
        .map(|obj| obj.keys().any(|k| BROWSER_STEPS.contains(&k.as_str())))
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// 静态校验测试
// ─────────────────────────────────────────────────────────────────────────────

/// 最重要的测试：所有 adapter yaml 文件都必须能被正确解析。
/// 任何 yaml 语法错误、缺少必填字段都会在这里被发现。
#[test]
fn all_adapters_parse_without_error() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    assert!(!files.is_empty(), "No yaml files found in adapters/");

    let mut failed: Vec<String> = Vec::new();

    for path in &files {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));

        if let Err(e) = parse_yaml_adapter(&content) {
            let rel = path.strip_prefix(&dir).unwrap_or(path);
            failed.push(format!("  {}: {}", rel.display(), e));
        }
    }

    if !failed.is_empty() {
        panic!(
            "{}/{} adapters failed to parse:\n{}",
            failed.len(),
            files.len(),
            failed.join("\n")
        );
    }

    println!("✓ {} adapters all parsed OK", files.len());
}

/// cookie/extension strategy 的 adapter 应该有 domain 字段。
/// 没有 domain 会导致浏览器无法确定注入 cookie 的目标站点。
#[test]
fn cookie_strategy_adapters_have_domain() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    let mut missing: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cmd) = parse_yaml_adapter(&content) else {
            continue; // 解析失败的另一个测试会抓到
        };

        use opencli_rs_core::Strategy;
        // Cookie/Ui/Intercept strategy 都需要 domain 来确定 cookie 注入目标
        let needs_domain = matches!(
            cmd.strategy,
            Strategy::Cookie | Strategy::Ui | Strategy::Intercept
        );
        if needs_domain && cmd.domain.is_none() {
            let rel = path.strip_prefix(&dir).unwrap_or(path);
            missing.push(format!("  {} ({} {})", rel.display(), cmd.site, cmd.name));
        }
    }

    if !missing.is_empty() {
        panic!(
            "{} cookie/extension adapters missing 'domain' field:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}

/// 所有 adapter 都必须有非空的 description。
/// 没有描述的 adapter 无法被 FTS 搜索发现，也无法在帮助信息里展示。
#[test]
fn all_adapters_have_non_empty_description() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    let mut missing: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cmd) = parse_yaml_adapter(&content) else {
            continue;
        };

        if cmd.description.trim().is_empty() {
            let rel = path.strip_prefix(&dir).unwrap_or(path);
            missing.push(format!("  {} ({} {})", rel.display(), cmd.site, cmd.name));
        }
    }

    if !missing.is_empty() {
        panic!(
            "{} adapters have empty description:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}

/// Pipeline 里每个 step 必须是单 key 的 object。
/// 多个 key 或者不是 object 会导致 execute_pipeline 运行时 panic。
#[test]
fn pipeline_steps_are_valid_single_key_objects() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    let mut bad: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cmd) = parse_yaml_adapter(&content) else {
            continue;
        };

        let Some(pipeline) = cmd.pipeline else {
            continue;
        };

        for (i, step) in pipeline.iter().enumerate() {
            let valid = step.as_object().map(|obj| obj.len() == 1).unwrap_or(false);
            if !valid {
                let rel = path.strip_prefix(&dir).unwrap_or(path);
                bad.push(format!(
                    "  {} ({} {}) step[{}]: {}",
                    rel.display(),
                    cmd.site,
                    cmd.name,
                    i,
                    step
                ));
            }
        }
    }

    if !bad.is_empty() {
        panic!(
            "{} pipeline steps are not single-key objects:\n{}",
            bad.len(),
            bad.join("\n")
        );
    }
}

/// Required 参数必须有 description。
/// 否则用户看到 required 参数却不知道该填什么。
#[test]
fn required_args_have_description() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    let mut missing: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cmd) = parse_yaml_adapter(&content) else {
            continue;
        };

        for arg in &cmd.args {
            if arg.required
                && arg
                    .description
                    .as_ref()
                    .map(|d| d.trim().is_empty())
                    .unwrap_or(true)
            {
                let rel = path.strip_prefix(&dir).unwrap_or(path);
                missing.push(format!(
                    "  {} ({} {}) arg '{}'",
                    rel.display(),
                    cmd.site,
                    cmd.name,
                    arg.name
                ));
            }
        }
    }

    if !missing.is_empty() {
        panic!(
            "{} required args missing description:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}

/// Columns 必须非空（有 pipeline 的 adapter）。
/// 没有 columns 定义时 Table/CSV 输出无法确定列顺序。
#[test]
fn adapters_with_pipeline_have_columns() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    let mut missing: Vec<String> = Vec::new();

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cmd) = parse_yaml_adapter(&content) else {
            continue;
        };

        if cmd.pipeline.is_some() && cmd.columns.is_empty() {
            let rel = path.strip_prefix(&dir).unwrap_or(path);
            missing.push(format!("  {} ({} {})", rel.display(), cmd.site, cmd.name));
        }
    }

    if !missing.is_empty() {
        panic!(
            "{} adapters have pipeline but no columns:\n{}",
            missing.len(),
            missing.join("\n")
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pipeline 集成测试
//
// 思路：解析真实 adapter yaml，注入 mock 数据替代浏览器/网络步骤，
// 只执行 transform 步骤（map/filter/limit/sort/select），
// 验证输出结构与 columns 一致。
// ─────────────────────────────────────────────────────────────────────────────

/// 用于注入 mock 初始数据的假 step。
/// 不管 pipeline 之前有什么，直接把 mock_data 作为初始 data 返回。
struct InjectStep(Value);

#[async_trait]
impl StepHandler for InjectStep {
    fn name(&self) -> &'static str {
        "__inject__"
    }

    async fn execute(
        &self,
        _page: Option<Arc<dyn IPage>>,
        _params: &Value,
        _data: &Value,
        _args: &HashMap<String, Value>,
    ) -> Result<Value, CliError> {
        Ok(self.0.clone())
    }
}

/// 构建测试用的 StepRegistry：只包含 transform 步骤 + inject 步骤。
fn make_test_registry(mock_data: Value) -> StepRegistry {
    let mut registry = StepRegistry::new();
    register_transform_steps(&mut registry);
    registry.register(Arc::new(InjectStep(mock_data)));
    registry
}

/// 把 yaml pipeline 里的浏览器/网络步骤全部移除，
/// 在最前面注入 mock 数据，保留所有 transform 步骤。
///
/// 例：[evaluate, map, fetch, limit] → [__inject__, map, limit]
fn build_test_pipeline(pipeline: &[Value], mock_data: &Value) -> Vec<Value> {
    let mut result = vec![serde_json::json!({ "__inject__": mock_data })];
    for step in pipeline {
        if !is_browser_step(step) {
            result.push(step.clone());
        }
    }
    result
}

// ── bilibili/hot ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bilibili_hot_transform_pipeline() {
    let yaml_path = adapters_dir().join("bilibili/hot.yaml");
    let content = std::fs::read_to_string(&yaml_path)
        .unwrap_or_else(|_| panic!("Cannot read {:?}", yaml_path));
    let cmd = parse_yaml_adapter(&content).unwrap();

    // evaluate 步骤返回的数据结构（mock）
    let mock_data = serde_json::json!([
        {"title": "测试视频1", "author": "UP主A", "play": 1_000_000, "danmaku": 5000},
        {"title": "测试视频2", "author": "UP主B", "play": 500_000,  "danmaku": 2000},
        {"title": "测试视频3", "author": "UP主C", "play": 300_000,  "danmaku": 1000},
        {"title": "测试视频4", "author": "UP主D", "play": 100_000,  "danmaku": 500},
        {"title": "测试视频5", "author": "UP主E", "play": 50_000,   "danmaku": 100},
    ]);

    let pipeline = cmd
        .pipeline
        .expect("bilibili/hot.yaml should have a pipeline");
    let test_pipeline = build_test_pipeline(&pipeline, &mock_data);

    let mut args = HashMap::new();
    args.insert("limit".to_string(), serde_json::json!(3));

    let registry = make_test_registry(mock_data);
    let result = execute_pipeline(None, &test_pipeline, &args, &registry)
        .await
        .unwrap();

    let arr = result.as_array().expect("output must be an array");

    // limit 3 — 超出部分被截断
    assert_eq!(arr.len(), 3, "limit should truncate to 3 items");

    // 验证输出字段与 columns 定义一致
    // columns: [rank, title, author, play, danmaku]
    for (i, item) in arr.iter().enumerate() {
        let obj = item.as_object().expect("each item must be an object");
        assert!(obj.contains_key("rank"), "item[{i}] missing 'rank'");
        assert!(obj.contains_key("title"), "item[{i}] missing 'title'");
        assert!(obj.contains_key("author"), "item[{i}] missing 'author'");
        assert!(obj.contains_key("play"), "item[{i}] missing 'play'");
        assert!(obj.contains_key("danmaku"), "item[{i}] missing 'danmaku'");

        // rank 应该是 index + 1
        assert_eq!(
            obj["rank"].as_i64(),
            Some((i + 1) as i64),
            "item[{i}] rank should be {}",
            i + 1
        );
    }

    // 验证 columns 定义本身
    assert!(cmd.columns.contains(&"rank".to_string()));
    assert!(cmd.columns.contains(&"title".to_string()));
}

// ── bilibili/ranking ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_bilibili_ranking_transform_pipeline() {
    let yaml_path = adapters_dir().join("bilibili/ranking.yaml");
    if !yaml_path.exists() {
        return; // adapter 不存在则跳过
    }

    let content = std::fs::read_to_string(&yaml_path).unwrap();
    let cmd = parse_yaml_adapter(&content).unwrap();
    let Some(pipeline) = cmd.pipeline else { return };

    // 通用 mock 数据：模拟视频列表
    let mock_data = serde_json::json!([
        {"title": "视频A", "author": "UP1", "play": 999, "score": 100},
        {"title": "视频B", "author": "UP2", "play": 888, "score": 90},
    ]);

    let test_pipeline = build_test_pipeline(&pipeline, &mock_data);
    let args: HashMap<String, Value> = {
        let mut m = HashMap::new();
        m.insert("limit".to_string(), serde_json::json!(2));
        m
    };

    let registry = make_test_registry(mock_data);
    let result = execute_pipeline(None, &test_pipeline, &args, &registry).await;

    // 只要不 panic/返回 error 就行——transform 步骤语义正确
    assert!(result.is_ok(), "pipeline failed: {:?}", result.err());
    let arr = result.unwrap();
    assert!(arr.is_array(), "output must be array");
}

// ── 通用：所有只含 transform 步骤的 adapter 的 pipeline 在 mock 数据下能正确运行 ──

#[tokio::test]
async fn transform_only_pipelines_run_with_mock_data() {
    let dir = adapters_dir();
    let files = collect_yaml_files(&dir);
    let mut failed: Vec<String> = Vec::new();
    let mut tested = 0;

    for path in &files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(cmd) = parse_yaml_adapter(&content) else {
            continue;
        };
        let Some(pipeline) = cmd.pipeline else {
            continue;
        };

        // 只测试：过滤掉浏览器/网络步骤后，剩下的都是已知 transform 步骤的 adapter
        // 注意：select 步骤依赖数据结构，不在通用 mock 测试范围内（由专项测试覆盖）
        let known_transform = ["map", "filter", "limit", "sort"];
        let non_browser_steps: Vec<&Value> =
            pipeline.iter().filter(|s| !is_browser_step(s)).collect();

        let all_transform = non_browser_steps.iter().all(|s| {
            s.as_object()
                .map(|obj| obj.keys().all(|k| known_transform.contains(&k.as_str())))
                .unwrap_or(false)
        });

        if !all_transform || non_browser_steps.is_empty() {
            continue; // 有自定义步骤或没有 transform 步骤，跳过
        }

        tested += 1;

        // 通用 mock 数据：足够覆盖大多数 map 模板
        let mock_data = serde_json::json!([
            {
                "title": "item 1", "name": "item 1", "author": "author A",
                "rank": 1, "score": 100, "play": 1000, "danmaku": 50,
                "url": "https://example.com/1", "id": "id_1",
                "description": "desc 1", "date": "2026-01-01",
                "views": 1000, "likes": 100, "comments": 10,
                "deleted": false, "active": true, "tags": ["tag1", "tag2"]
            },
            {
                "title": "item 2", "name": "item 2", "author": "author B",
                "rank": 2, "score": 80, "play": 800, "danmaku": 30,
                "url": "https://example.com/2", "id": "id_2",
                "description": "desc 2", "date": "2026-01-02",
                "views": 800, "likes": 80, "comments": 8,
                "deleted": false, "active": true, "tags": ["tag3"]
            },
        ]);

        let test_pipeline = build_test_pipeline(&pipeline, &mock_data);
        let mut args: HashMap<String, Value> = HashMap::new();
        // 注入所有可能的 args（使用 adapter 定义的 defaults）
        for arg in &cmd.args {
            if let Some(default) = &arg.default {
                args.insert(arg.name.clone(), default.clone());
            } else {
                // required 参数给个合理的默认值
                args.insert(arg.name.clone(), serde_json::json!("test"));
            }
        }
        // 保证 limit 参数存在
        args.entry("limit".to_string())
            .or_insert_with(|| serde_json::json!(10));

        let registry = make_test_registry(mock_data);
        if let Err(e) = execute_pipeline(None, &test_pipeline, &args, &registry).await {
            let rel = path.strip_prefix(&dir).unwrap_or(path);
            failed.push(format!(
                "  {} ({} {}): {}",
                rel.display(),
                cmd.site,
                cmd.name,
                e
            ));
        }
    }

    if !failed.is_empty() {
        panic!(
            "{}/{} transform-only pipelines failed with mock data:\n{}",
            failed.len(),
            tested,
            failed.join("\n")
        );
    }

    if tested > 0 {
        println!("✓ {tested} transform-only pipelines ran OK with mock data");
    }
}

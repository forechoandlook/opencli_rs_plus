# 2026-06-26

## 代码精简与解耦
- **删除孤儿 crate `opencli-rs-external`**：CLAUDE.md 已声明「不再做执行代理」，且全工程零引用，从 workspace 移除。
- **删除未接线的 `opencli-rs-ai` crate**：仅在 `cli/Cargo.toml` 声明依赖、无任何 `use`，一并移除（如需 AI 能力后续以 feature flag 重新引入）。
- **删除 5 个零使用的浏览器 UI step**：`click` / `type` / `press` / `screenshot` / `snapshot`，402 个 adapter 中均未使用（项目走 API/evaluate 路线）。`evaluate` 仍是主力（355 处）。
- **合并重复的 dump 工具函数**：`resolve_dump_path` / `api_dump_enabled` / `sanitize_dump_part` / `dump_value_to_file` / `dump_api_response` 此前在 `fetch.rs`、`browser.rs`、`transform.rs` 三处重复，统一抽到 `steps/dump.rs`。
- **解开 daemon → cli 反向依赖**：将 `execute_command` 执行引擎下沉到新建的 `opencli-rs-engine` crate；daemon 的 `scheduler`/`socket` 库代码改为依赖 engine，不再依赖 cli（仅 daemon 二进制 `main.rs` 仍组合 `cli::runner`，属正常的二进制级组合）。

## 知乎收藏夹修复
- **修复 `zhihu collection_items_api` 在混合类型收藏夹下返回空**：删除把整个数据数组内联进 JS 源码做空检查的 `evaluate` 步骤（大收藏夹会拼炸）。
- 新增 `zhihu my_collections`：列出当前登陆用户的收藏夹。
- 备注：知乎收藏接口单页 `limit` 上限为 20，更多条目需用 `offset` 翻页（`--limit 50` 会返回空）。

## 新增公共 API 源（无需登陆）
- `npm search` / `npm info`：npm 包搜索与详情（`/latest` 端点避免拉取多 MB 全量文档）。
- `pypi info`：PyPI 包详情。
- `crates search` / `crates info`：crates.io Rust 包搜索与详情（带 User-Agent 头）。
- 约定记录：`${{ }}` 模板引擎不支持函数调用（`encodeURIComponent`、`.slice()`）和可选链 `?.`；`fetch` step 返回的 JSON 不带 `body.` 前缀（仅 `bg_fetch` 带）。

# 2026-04-15

## Qwen Adapters Enhancement
- **API Exploration**: 
  - Discovered Qwen REST API endpoints: `/api/v2/configs`, `/api/v2/chats`, `/api/v2/chat/messages`, `/api/v2/chat/history`, `/api/v2/files`, `/api/v2/chat/completions`
  - Confirmed API authentication via localStorage token
  - Documented API structure in `docs/qwen-api-reference.md`
  
- **Session Export Feature**: 
  - Added new `qwen export` adapter to export chat conversations to JSONL format
  - Supports exporting from conversation URLs (e.g., https://chat.qwen.ai/c/<chat_id>)
  - Extracts messages via DOM parsing with API fallback
  - Creates JSONL files with structure: `{"role":"user|assistant","content":"...","timestamp":"...","chat_id":"..."}`
  - Documentation in `docs/qwen-session-export.md`

- **Session Persistence**: 
  - Added `persistent: true` to all Qwen adapters to enable browser session reuse
  - Added session validation checks (localStorage token and device ID) to all adapters
  - Improved wait times from 2 to 3 seconds for better reliability
- Original Qwen feature adapters: `check`, `deep-research`, `web-dev`, `learn`, `travel`, `artifacts`, `search`, `slides`, and `video`, with session-aware `missing/disabled/login_required` status reporting.
- Qwen image-generation adapter for `opencli qwen image`, with login-aware fallback
- Downloadable Qwen resource adapters indexing public assets
- Qwen menu adapters for public feature/entry capabilities
- Qwen public API status adapters using browser-side fetch

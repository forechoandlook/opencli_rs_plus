# 2026-08-04

## 浏览器 Adapter 静默化与加速

- **所有浏览器 adapter 都改为隔离静默窗口执行**：扩展不再把复用 Chrome 窗口中的任意现有标签当作自动化目标；自动化只会使用一个最小化的 OpenCLI 自有窗口及后台页。因此包含 `navigate` 的 pipeline 不会刷新、抢占或在用户当前窗口标签栏中新增网站标签。
- **按同源复用会话页**：执行引擎以 adapter 配置的 `domain` 作为页面 workspace；同一网站的不同命令会复用已建立的浏览器连接、标签和站点缓存，减少建页与加载开销；后台标签始终不激活。
- **回归覆盖**：扩展测试现在验证即使自动化窗口是用户已有窗口，也一定新建 OpenCLI 标签而不会复用用户标签。
- **扩展任务面板**：点击浏览器插件即可看到最近 30 个浏览器 adapter 任务、运行状态、完成/失败时间和结果摘要；摘要只留在 browser-daemon 内存中，字段名含 token、cookie、password、secret 或 authorization 的值会被脱敏，且文本长度受限。
- **任务面板 CORS**：`/tasks` 支持 Chrome Extension 的 `OPTIONS` 预检与带 `X-OpenCLI` 的读取请求；仅 `chrome-extension://` 来源会获得跨域响应头，普通网页不能读取本地任务结果。
- **任务面板免预检**：扩展读取 `/tasks` 不再附带非必要自定义 header，避免 Chrome 对本地任务列表发起预检；daemon 改为校验 `chrome-extension://` Origin，内部客户端仍可使用 `X-OpenCLI`。
- **本地 release 构建安装**：新增 `scripts/build-release.sh`，本地执行 release 构建后自动安装 `opencli` 到 `~/.local/bin/opencli`，方便立即验证最新二进制。
- **`bg_fetch` 登录态修复**：不再尝试设置浏览器禁止的 `Cookie` 请求头，改用 `credentials: 'include'` 让 Chrome 按规则携带目标站点 Cookie，避免部分站点出现 `TypeError: Failed to fetch`。
- **`bg_fetch` CORS 回退**：当站点拒绝扩展 service worker 的跨域请求时，自动在 OpenCLI 最小化窗口的同源页面中重试；保留一方登录态并避开 CORS，不会打开或刷新用户已有标签。
- **知乎热榜加速**：`zhihu hot` 直接选择同源请求，避免一次必然失败的扩展跨域尝试；同源 API 导航只等待 URL 提交，不等待知乎页面的完整资源加载，消除约 15 秒的 `complete` 等待。
- **同源缓存命中不再伪导航**：当缓存页已处于目标 URL（包括 `/` 尾斜杠等价形式）时，扩展直接复用当前文档；此前 Chrome 不会为相同 URL 发出 URL-change 事件，导致每次命中缓存仍会落入 15 秒导航兜底。
- **同源 API 按 origin 复用**：`bg_fetch.same_origin` 不再要求缓存页处于特定路由；只要协议、域名和端口相同，就直接在已有文档中发起 fetch。仅 origin 变化时才导航到上下文页。
- **小红书 Feed 保守热缓存**：`xiaohongshu feed` 首次导航仅等待 document commit，缓存页已在 `/explore` 时跳过重复导航；站内 Pinia action 的成功响应在该自动化页内缓存 60 秒，命中缓存不会再次调用 `fetchFeeds`。保留 8 秒网络等待，并在最多 3 秒 hydration 窗口内等待 store 就绪，避免为加速而更频繁请求平台接口。
- **小红书签名详情链接**：`xiaohongshu feed` 现在保留每个 Feed 项页面实际提供的 `xsecToken`，输出 `xsec_token` 与 `xsec_source=pc_feed` 的完整详情 URL；`xiaohongshu download` 同时接受笔记 ID 或完整 `/explore/<id>` URL，完整 URL 会原样保留其 `xsec_*` 参数。不会伪造、跨笔记复用或持久化签名 token。
- **浏览器风控保护**：浏览器 step 支持 `retry: false`，供存在请求副作用或平台风控风险的 adapter 禁用自动重试。小红书 Feed 和详情导航已启用该选项；任务面板预览会脱敏 URL 中的 `xsec_token`，CLI 输出仍保留原始、可用链接。
- **共享 LRU 页面缓存**：浏览器 adapter 改为共用一个最小化自动化窗口，最多缓存 10 个实际后台页面；同源请求直接复用已打开的后台页，超出容量时按页面访问时间 LRU 淘汰。扩展弹窗新增页面缓存列表，显示当前占用、来源和 URL。
- **小红书下载产物完整化**：`xiaohongshu download` 现在在下载目录写入 `note.md`，包含标题、作者、正文、笔记类型、脱敏后的来源链接和每个媒体的下载状态；图片优先使用页面状态的原始 CDN URL（不再擅自移除 query），并按响应 Content-Type 保存正确扩展名。`media-batch` 改为流式落盘并采用 `.part` 后原子改名，避免视频等大文件被一次性读入内存。
- **小红书视频直链复用**：播放器可能把 DOM `src` 换成 `blob:`/MediaSource，但页面的 Resource Timing 仍保留其已经请求过的、短期签名的 XHS CDN MP4 URL。下载前现在只读当前自动化页的该资源记录（限制为 `sns-video*.xhscdn.com` 的 MP4），并复用它；没有记录时才最多轮询 DOM 约 2.5 秒。不点击播放、不额外请求接口、不伪造签名。始终只有 `blob:`/MediaSource 或 HLS (`.m3u8`) 时明确标记为 `unsupported`，仍会保存正文 `note.md`，不会规避站点视频流保护。
- **小红书被动评论与热推**：新增 `xiaohongshu comments <完整签名链接>`，只读取详情页首屏已携带的评论状态，默认最多 10 条、无滚动、无翻页、无评论 API 调用；新增 `xiaohongshu hot`，只读取首页已加载的 Feed 状态，不调用 Pinia `fetchFeeds` 或主动刷新。页面未自然提供数据时会明确失败，而不是为凑结果增加请求频率。

# 2026-07-26

## 下载类 adapter 空结果不再静默成功
- **`download` step 的 `media-batch`/`twitter-media` 分支**：`items` 为空数组时此前会返回一行 `status: failed` 的展示数据但 pipeline 本身仍是 `Ok`，daemon job 因此被标成 `done`，队列/审计里看不出失败。现在改为直接返回 `CliError`，job 会落到 `failed` 状态并按指数退避重试，`opencli job show <id>` 能看到真实 error。
- **`adapters/xiaohongshu/download.yaml` 修复两个问题**：
  1. evaluate 里收集的字段一直叫 `media`，但 `download` step 读的是 `data.items`，且 params 没传 `type`，实际走的是"仅返回单个 url 元数据"的默认分支——从未真正批量下载过。改为 `items` 字段 + `type: media-batch`，接入真正的批量下载逻辑。
  2. 没抓到任何图片/视频时（笔记下架/需要登录/风控）新增显式 `throw`，让失败在 evaluate 阶段就暴露，而不是产出空数组静默"成功"。
- **`adapters/twitter/download.yaml`** 同样把"空媒体返回一行 failed 展示数据"改为 `throw`，走真实失败路径。
- 说明：项目目前没有针对验证码/滑块等平台风控的专门检测逻辑，出现风控时会表现为选择器抓不到数据，现在至少能通过上述改动转化为可重试、可审计的失败，而不是空结果。

# 2026-06-26

## 合并 summaries 到 adapters（summary 自动派生）
- **删除独立 `summaries/` 目录**：summary 不再单独维护，改为运行时从 adapters 动态生成。
- **每个 adapter 新增 `meta.yaml`**（`name`/`description`/`version`），承载 adapter 级整体描述；工具列表由各 tool yaml 的 `name`+`description` 自动拼出。已为全部 79 个 adapter 批量生成（55 个沿用旧 summary 描述，24 个无 summary 的从工具描述派生，可后续手工润色）。
- `summary list` / `summary show` 改为读取 `meta.yaml` + 扫描 tool yaml 实时生成，输出格式与旧 summary 完全一致。
- FTS 索引的 `summary` 列改取 `adapters/<site>/meta.yaml`（替代旧 `summaries/<site>.md`），增量 sync 依据 `meta.yaml` mtime。
- adapter discovery 扫描时跳过 `meta.yaml`，不再误当作工具解析。

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

# 2026-08-10

## 再精简：history / sync / hidden / socket exec / 顶层 status

- **删除** `opencli history` 与 `history.jsonl` 落盘（engine 不再写审计日志）。
- **删除** `adapter sync`、`hidden` 设置维度（仅保留 disable）。
- **删除** socket `exec` 远程跑 adapter、裸 `socket` CLI 调试口。
- 顶层 `status` / `stop` / `restart` 收敛为 **`opencli daemon status|stop|restart|start`**。
- 可清理本机残留：`jobs.db`、`tools/`、`summaries/`、`feedback.jsonl`、`issues.db`、`history.jsonl`。
- **CI/Release**：adapter corpus 跳过 `meta.yaml`；release 不再打包已移除的 `summaries/`。

## adapter enable/disable 真正可用

- 设置落到 `~/.opencli-rs/adapter_settings.json`（`opencli-rs-core::AdapterSettings` 共用）。
- **direct 模式**也会过滤 disabled：不出现在 help，也不能 `opencli <site> <cmd>` 执行。
- `adapter enable/disable` **不依赖 daemon**（daemon 在线走 socket，离线直接写文件）。
- 支持 `site command`、`site/command`；**裸 site 名禁用整站**。

## 搜索降级：去掉 FTS / 热榜 / find

- **删除** SQLite FTS5 索引（`index.db`）、BM25、usage 统计、socket `adapter.hot` / `trending` / `reindex`，以及独立命令 `opencli find`。
- **唯一入口** `opencli adapter search`：内存子串匹配（name/site/description/domain），命中时打印可复制用法；daemon 不可用时本地扫描。
- 数百级 adapter 规模不需要全文检索引擎；发现靠 search + `opencli <site> --help`。

## 移除定时 job 调度

- **删除** `opencli job *`、`JobStore`（`jobs.db`）、`Scheduler` 轮询循环及 socket `job.*` API。
- daemon 职责收敛为：adapter/plugin 管理、扩展 action 发现、status/socket；不再做间隔任务调度。
- 去掉 `--poll-interval` / `--db` 启动参数；需要定时时用系统 cron/launchd 调用 adapter 命令。

## 精简产品面：移除 tools / summary / feedback

- **删除** `opencli tools`（外部 CLI 知识库）、`opencli summary`（adapter 文案浏览）、`opencli feedback`（本地反馈/开 issue）及其实现与文档。
- `opencli find` 仅搜索 adapter 并打印可复制用法，不再混入 tools。
- 索引里的 FTS `summary` 列仍来自各站 `meta.yaml`（检索用文本），与已删除的 `summary` 命令无关。
- 产品重心回到 **adapter 质量**（YAML、回归、plugin 分发、KV 身份缓存）。

## CLI 帮助补全 kv / history / find

- `opencli --help` 此前未展示已实现的 `kv`、`history`、`find`（client 子命令已存在）。现已并入主 CLI 帮助树，避免功能「能跑但不可发现」。

## adapter 分发仓库

- 新增独立仓库 [forechoandlook/opencli-adapters](https://github.com/forechoandlook/opencli-adapters) 作为 YAML adapter 的 plugin 分发源（`opencli plugin install forechoandlook/opencli-adapters` / `plugin update`）。
- opencli-rs 侧增加 `scripts/sync-adapters-repo.sh`，将本仓 `adapters/` 同步到 opencli-adapters 检出目录；开发仍以 monorepo `adapters/` 为准，远端仓用于用户安装与版本 tag。

## opencli KV + 小红书 favorites 身份解析改造

- **新增本地 KV 存储**（`~/.opencli-rs/kv.json`）：用于跨命令缓存稳定的身份/小状态（如 `xiaohongshu:me.userId`），不存 cookie/token，默认不缓存整份抓取结果。CLI：`opencli kv get|set|list|del|clear`；支持可选 TTL（`30d`/`24h`/`15m`/`60s` 或秒数）；`clear` 全库需 `--all`。
- **新增 pipeline step**：`kv_get` / `kv_set`。`kv_get` 支持 `field` + `only_if_empty` 合并进当前 data；`kv_set` 默认跳过空值且不改动 pipeline data。
- **重写 `xiaohongshu favorites`**：不再绕 `creator.xiaohongshu.com` 取 `red_num`。解析顺序为 CLI `--user-id` → KV `xiaohongshu:me.*` → 主站 `__INITIAL_STATE__.user.userInfo`；解析成功后写回 KV；直达 `?tab=fav&subTab=note` 读首屏收藏。换号时以页面 userId 覆盖缓存。
- **B 站 me mid 接入 KV**：`bilibili me` 每次从 nav 解析后写入 `bilibili:me.mid` / `bilibili:me.uname`；`favorites` / `favorite` / `following`（无 uid 时）优先读 KV，miss 再打 nav；业务失败且疑似换号时回退 nav 并覆盖缓存。不缓存收藏内容本身。


# 2026-08-05

## 闲鱼搜索 adapter

- 修复 `xianyu search` 对当前商品链接的识别：闲鱼会将 `spm` 跟踪参数置于 `id` 之前，现按 `/item?` 路由匹配后再解析商品 ID，避免真实搜索结果被误报为空。
- 新增只读的 `xianyu feed`、`hot`、`messages` 和 `conversation`：分别读取首页猜你喜欢、首页展示的热门搜索词、已加载会话摘要/未读数，以及聊天页当前会话的已加载消息。不会发送私信、操作订单、购买、收藏或翻页加载历史消息。
- 新增 `xianyu download <item_id>`：只下载商品详情响应中已展示的 `imageInfos` 公共图片，默认写入 `./xianyu-downloads` 并生成 `item.md` 元数据；`xianyu item` 同步输出 `image_count`，便于从 feed 结果选择有图片的商品后下载。
- `xianyu messages` 改为复用消息页真实调用的 `mtop.taobao.idlemessage.pc.session.sync`（`v: 3.0`），稳定输出会话 ID、类型、对方昵称、最新摘要、时间和每会话未读数；新增 `xianyu unread`，复用 `pc.redpoint.query` 返回消息总未读数。两者均为只读请求，不会切换会话、标记已读或发送消息。
- 新增 `xianyu publish`：通过浏览器原生 CDP 文件输入能力上传本机图片，填写描述、售价、可选原价与运费后才点击闲鱼发布按钮。该命令强制要求 `--confirm true`，并在上传或写入页面前校验；没有确认不会修改表单。扩展/浏览器桥新增通用 YAML `upload` step，使用 `DOM.setFileInputFiles`，不把站点上传逻辑硬编码进扩展，也不伪造闲鱼的最终发布 API。
- 重构 `xiaohongshu publish`：删除旧流程“忽略 `--images` 且直接发布”的问题，改为在新版创作页切换“上传图文”、通过通用 `upload` step 上传真实本地图片，再填写标题/正文。该命令要求 `--confirm true`；默认 `--draft true`，显式 `--draft false` 才会尝试公开发布。话题以 `#话题` 追加进正文，未调用或伪造未验证的发布 API。
- 新增 B 站 `publish-status` 与 `publish`：前者只读确认投稿页/视频上传控件；后者使用浏览器原生文件上传填写标题和简介。`publish` 强制 `--confirm true`，并默认 `--submit false`；只有同时显式传 `--submit true` 才会点击“立即投稿”，不会伪造投稿 API。
- B 站 `download` 不再依赖 `yt-dlp`：仅复用视频页自然提供的 DASH 音视频地址，以页面 Referer/User-Agent 下载后由本机 `ffmpeg` 合成为单个 MP4；不伪造播放签名或调用未验证下载 API。新增通用 YAML `dash-mux` step，供任何页面已提供 DASH 地址的 adapter 复用。
- 新增 `bilibili comments <BV号>`：读取视频首屏顶层评论，支持热门/最新排序和最多 50 条限制；不点赞、回复、加载二级回复或翻页。
- 修复 `bilibili favorite`：先读取当前登录 UID 再请求收藏夹，修正 `up_mid=0` 导致误报为空的问题；新增 `bilibili favorites` 列出全部收藏夹（ID、名称、数量和可见性），`favorite --folder_id <ID>` 可读取指定收藏夹内容。两项均为只读。
- 新增 `xiaohongshu favorites`：解析当前登录用户并打开 `?tab=fav&subTab=note` 读取首屏收藏（实现已于 2026-08-10 改为主站 userInfo + KV，不再依赖创作者中心）。保留每项页面提供的 `xsec_token` 链接，不伪造签名、不翻页、不执行收藏或取消收藏。扩展在该页面可提供“导出当前个人收藏”动作。

## 主流游戏素材源接入

- 新增 `opengameart search`、`kenney category`、`itchio assets`、`ambientcg popular` 与 `freesound search`，分别覆盖开放游戏素材、CC0 资源包、独立创作者市场、PBR 材质和音效发现。
- Kenney 与 ambientCG 的输出明确标记为 `CC0`/`commercial_safe: true`。OpenGameArt、itch.io 和 Freesound 的许可证逐条不同，输出强制标记为需复核，不会把免费或公开页面误标为可直接商用。
- Unity Asset Store 的网页 adapter 保持浏览、榜单和价格发现边界。官方将已购或免费获取的 Asset Store 包交付给同一 Unity ID 下的 Package Manager，因此不伪造或绕过其下载交付流程。

## Unity Asset Store adapter

- 新增 `unity-assets hot`、`unity-assets search` 与 `unity-assets info`。`hot` 读取首页已经展示的推荐资源 ID 后经站内详情接口补全数据；`search` 使用站点原生搜索路由及其专用结果容器，再用资源详情接口补全数据，不会从页面底层推荐卡片猜测结果；`info` 用资源数字 ID 获取详情。
- 三个命令都仅执行读取请求，不会购买、添加收藏或修改账户。价格按 `--currency` 传入币种（默认 `CNY`）；`search` 支持 `--min_price` 与 `--max_price`，按同一显示币种筛选，避免把首页内容误报为搜索结果。
- `hot` 现改为全站畅销排序；新增 `top <free|paid|new>`、`category-hot <category>`、`category-new <category>` 和 `sale`。支持的分类为 `sdk`、`3d`、`2d`、`audio`、`tools`、`vfx`、`templates`、`add-ons`，可分别用于技术方案、玩法模板与美术音频素材的灵感收集。
- Unity 资源现在明确输出 `current_price`、`original_price`、`discount_percent`、`currency` 与评分数量，便于优先筛选免费、高折扣和高口碑素材。
- 新增 `unity-assets download-media <id>`：下载资源页公开截图与外部视频缩略图，并生成 `asset.md` 元数据。视频只保留页面提供的外部链接和缩略图；不下载资源包本体，不提取 `blob:`/HLS 或受保护视频流。
- 统一榜单、搜索与详情的分类和评分输出：分类对象改为名称/slug，评分始终为数值（包含 `0` 分资源）。
- 榜单在站点自动注入语言路径或将筛选条件改写为 hash 路由时，不再因 URL 字面值变化误报失败；仍要求实际解析到资源 ID 才会成功。
- 新增 `unity-assets my-assets`，通过当前 Unity ID 的授权列表读取已拥有资源，再补齐公开详情和价格字段；不下载或导入资源包。

## Steam 灵感与市场榜单

- 补充 `steam new-releases`、`steam coming-soon` 和 `steam specials`；既有 `steam top-sellers` 同步补充原价字段。四个 Steam 榜单可分别观察已验证的商业需求、新近出现的玩法、即将入场的题材和价格带。
- Steam 的 `price`/`original_price` 保持平台 API 返回的最小货币单位，以避免在缺少明确币种上下文时擅自换算。

# 2026-08-04

## 浏览器扩展当前页面操作

- **contextual adapter actions**：adapter YAML 可用 `context` 声明标题、URL 路径模式与 `activeTab` 计划；调度 daemon 在 loopback `127.0.0.1:10009` 仅提供 action 发现 API。点击后在用户已经打开的标签内直接、只读地解释 YAML 的 `evaluate`/`limit`/`download`，并调用浏览器下载，不会走 engine、browser-daemon 或任务面板。
- **小红书首批接入**：推荐首页 `/explore` 可在扩展中读取当前推荐 Feed；笔记详情页可直接启动“下载当前笔记”和“导出当前笔记首屏评论”。完整 URL 会原样传给 adapter，以保留页面提供的 `xsec_*` 签名参数。评论 adapter 仍保持被动首屏读取、无滚动和无翻页。
- **当前页字段兜底**：笔记下载在页面状态缺失正文时读取详情 DOM；评论在已加载状态为空时读取已渲染的首屏评论 DOM；Feed 按卡片提取标题、作者、点赞、类型、封面与签名链接。三者均不触发额外请求或滚动。
- **本地 API 边界**：当前页面 API 仅允许 `chrome-extension://` Origin；网页不能调用，也不能传入脚本、cookie 或页面 HTML。仅用户已启用的 adapter YAML 会作为当前页计划下发。
- **可见诊断**：弹窗显示当前 URL 的 host/path、匹配 action 数与 API 端口；查询失败会显示 HTTP 原因。相同的脱敏诊断会进入既有 extension log；以 `OPENCLI_VERBOSE=1` 启动 scheduler daemon 时，终端会记录 action 查询。

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

# 2026-08-08

## Daemon 进程管理、统一检索入口、请求审计日志

- **daemon 后台化 + 开机自启**：新增 `opencli daemon start/stop/restart/status/logs/config` 子命令组（`crates/opencli-rs-daemon/src/client.rs`）。`daemon start` 以 detach 方式后台启动（stdin `/dev/null`，stdout/stderr 重定向到 `~/.opencli-rs/daemon.log`），`daemon logs [-f] [-n N]` 查看日志，`daemon config` 打印 socket/db/log/pid/autostart 的解析结果。原有 `opencli daemon`（前台阻塞）、`opencli status/stop/restart` 保持不变，向后兼容。
  - 修复：`daemon.stop` 之前只是回一个 `{"stopping": true}` 的桩实现，进程从不退出；现在真正在响应发出后延迟 200ms 调用 `std::process::exit(0)` 并清理 pid 文件（`crates/opencli-rs-daemon/src/socket.rs`）。
- **开机自启**：`opencli daemon autostart install/uninstall/status`（`crates/opencli-rs-daemon/src/autostart.rs`），macOS 生成 `~/Library/LaunchAgents/com.opencli.daemon.plist` 并 `launchctl load -w`，Linux 生成 `~/.config/systemd/user/opencli-daemon.service` 并 `systemctl --user enable --now`。**未自动执行 install**（会修改系统开机行为），需用户手动运行。
- **统一检索入口**：新增 `opencli find <query>`，一次性搜索 adapters（站点抓取器）和 `opencli tools` 知识库并分别打印；adapter 命中会直接生成可复制的调用命令（含必填 positional 参数和可选 flag 及默认值，取自 `CliCommand.args`），不用再单独跑一次 `--help`。
- **请求/结果审计日志**：新增 `crates/opencli-rs-engine/src/history.rs`，在唯一的执行入口 `execute_command`（direct CLI 执行和 daemon 定时任务都走这里）落一条 JSON 到 `~/.opencli-rs/history.jsonl`：时间戳、adapter、参数、成功/失败、耗时、结果（截断 4000 字符）或错误。纯本地追加，不接 daemon。新增 `opencli history list [--limit N]` / `opencli history search <query>`。
- **撤销的方向**：这次同时做了云端任务拉取（daemon 轮询远程端点）和账号凭据 vault（先 OS keychain 后改本地文件），review 后都撤掉了——云端任务的真实需求只是"配置 base URL + key，本地任务自动往外推"，方向和轮询相反，没必要现在建一整套轮询/鉴权/去重的基础设施；vault 则是在没有明确需求下加的一层，直接被判定为过度设计。两者的代码、依赖（`reqwest`、`keyring`、`rpassword`）和 `jobs` 表的 `remote_id` 迁移都已完全移除。

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

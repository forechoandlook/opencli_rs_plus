0403 
- **版本号对齐**：workspace 版本已对齐到 `0.0.2`，与当前 `v0.0.2` tag 保持一致，`opencli --version` 会直接显示该版本号。
- **版本信息增强**：`opencli --version` 现在输出 `版本 + 短 commit hash`，例如 `opencli 0.0.2 (135b297e)`，便于定位构建来源；release workflow 增加 tag/version 一致性校验。
- **自卸载命令**：新增 `opencli uninstall`，在 Unix-like 系统上可直接删除当前二进制；Windows 下明确返回不支持提示，避免静默失败。
- **External CLI 执行代理已移除**：opencli-rs-cli 不再依赖 opencli-rs-external，external CLI 透传功能已删除。
- **`--version` 早退出**：顶层 `opencli --version` 现在在适配器发现之前直接返回，避免触发加载日志和 dev 输出，保证版本信息纯净输出。
- **工具知识库**：唯一入口是 `opencli tools`。本地纯文件方案从 `~/.opencli-rs/tools/*.md`（YAML frontmatter + Markdown body）加载，内存过滤，无 SQLite、无 daemon 依赖，支持 search / list / info / summary。
- **Desktop/Electron App 适配**：`strategy: ui` + `domain: localhost` 时走 CDP 直连模式（`crates/opencli-rs-browser/src/electron_apps.rs`），需目标 App 开启 `--remote-debugging-port`。内置端口映射：antigravity=9234, cursor=9226, codex=9222, chatwise=9228。用户可通过 `~/.opencli-rs/apps.yaml` 扩展。
- **Daemon Socket API**：JSON-RPC over TCP（127.0.0.1:10008），支持 daemon.*、job.*、adapter.* 方法。tools.* 已移除（本地直接执行）。
- **Scheduler 注意事项**：args 为 null 时正常处理；执行前自动注入 YAML 中定义的 default 参数；job ID 支持前缀匹配。

0409
- **原生命令扩展**：新增 `opencli update` / `opencli update --check`，支持检查 GitHub Release 并原地更新当前二进制。
- **反馈命令**：新增 `opencli feedback <title>`，默认写入 `~/.opencli-rs/feedback.jsonl`，加 `--open` 可打开预填好的 GitHub issue 页面。
- **帮助输出收敛**：`opencli --help` 默认只显示内置命令和 daemon/client 命令.
- **日志默认静默**：默认不打印 tracing 日志；通过 `OPENCLI_VERBOSE` 或 `RUST_LOG` 才开启输出，日志时间默认只显示到分钟，且可通过 `OPENCLI_LOG_TIME` 切换到秒、毫秒或关闭时间戳。
- **全局字段裁剪**：新增 `--fields a,b,c`，可在所有输出格式上只返回对象/对象数组的指定顶层字段。

0410-2
- **CLI 结构重构**：`runner.rs` 拆分为三个模块：`cli_builder.rs`（Command 树构建）、`dispatch.rs`（内置命令分发）、`runner.rs`（薄路由层）。消除了原先 800 行大杂烩。
- **`--help` 视觉分组**：通过 `display_order` 将子命令分成三组——TOOLS（本地无需 daemon）、AI FEATURES（需要浏览器）、DAEMON MANAGEMENT（需要 daemon）。`[daemon]` 前缀标注让用户一眼看出哪些命令依赖 daemon。去除了原先在 `after_help` 里手写的重复命令列表。
- **`job` 子命令帮助补全**：`run_at`、`delay`、`interval`、`args` 全部补充格式说明和示例；`cancel` vs `delete` 语义差异通过 long_about 区分（cancel 保留历史，delete 永久删除）。
- **`adapter search` 离线 fallback**：daemon 未运行时自动 fallback 到本地文件扫描（`discover_adapters` + 大小写不敏感子串匹配），结果尾部标注来源 `(daemon)` 或 `(local scan)`。

0410
- **版本来源收口**：`opencli update` 与发布安装链路不再依赖 GitHub API 的 `tag_name` 判定版本，统一改为读取 release 固定路径 `releases/latest/download/latest` 的纯文本版本文件。
- **下载入口固定化**：自更新与 `install.sh` 统一走 `releases/latest/download/<asset>` 固定路径，避免版本判断与资产下载来自不同来源。
- **版本号重置起点**：工作区版本已改回 `0.0.1`，为重新整理 tag/release 序列做准备。
- **知乎收藏夹 API 提速**：`zhihu collection_items_api` 改为使用 `bg_fetch` 直接在扩展后台发起请求并注入知乎 cookies，去掉 `navigate + 页面内 fetch`，避免知乎首页导航常见的 15 秒超时拖慢整条命令。
- **只读接口批量提速**：`zhihu search` 改为 `bg_fetch` 后台取数，避免为借 cookie 先开页面导致的额外导航等待；`zhihu feed_api` 与 `weibo hot` 实测存在 payload / 权限边界，暂不切换。
- **知乎搜索结果修正**：`zhihu search` 现在会同时解析标准 `search_result` 与顶部聚合卡片里的 `content_items/sub_contents`，避免 `--limit` 明明传了更大值却只返回少量结果。
- **API 调试 dump 开关**：新增 `OPENCLI_API_DUMP` / `OPENCLI_API_DUMP_DIR`，开启后自动落盘 `fetch` 与 `bg_fetch` 的原始响应，便于排查接口边界、保留调试证据和批量识别可提速的 adapter。
- **大响应与多端口连接修复**：浏览器 daemon 的 `/command` body limit 提升到 32MB，避免大 `bg_fetch` 结果触发 413；扩展与 popup 现在会主动扫描 `19825-19834`，自动选中可连的 daemon port 并保存，减轻多浏览器安装插件时的手动配置负担。
- **端口切换修正**：CLI 的浏览器入口不再在未设置 `OPENCLI_DAEMON_PORT` 时死绑 `19825`；扩展 popup 保存端口后会立即通知 background 断开旧连接并重连到新 port，避免“设置了 19826 但未生效”的假切换。
- **端口 pin 语义修正**：扩展区分自动探测端口与用户手工设置的端口；手工保存的 port 现在会被标记为 pinned，不再被自动扫描覆盖，避免多浏览器安装插件时两个 popup 都被重写到同一个端口。
- **Adapter 维护分类起步**：新增 `scripts/classify-adapters.sh` 与首版分类文档/清单，先把全库 adapter 按 `ui_automation`、`api_bg_fetch`、`api_page_fetch`、`api_direct_fetch`、`api_write_or_mutation`、`page_navigation_dom` 等类别收口，作为后续回归与巡检的基础。
- **首版本地回归 smoke**：新增 `docs/generated/regression-p1.tsv` 与 `scripts/regression-smoke.sh`，先覆盖高价值 API 类 adapter，输出 TSV/Markdown 回归结果并支持按 case 打开 API dump。
- **浏览器端口语义收紧**：CLI 浏览器桥接默认固定使用 `19825`，只有显式设置 `OPENCLI_DAEMON_PORT` 时才切到其他端口，不再在未指定时自动漂移到 `19826+`。

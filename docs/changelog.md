0403 
- **External CLI 执行代理已移除**：opencli-rs-cli 不再依赖 opencli-rs-external，external CLI 透传功能已删除。
- **工具知识库**：本地纯文件方案，从 `~/.opencli-rs/tools/*.md`（YAML frontmatter + Markdown body）加载，内存过滤，无 SQLite、无 daemon 依赖。`opencli tools` 子命令直接本地执行，支持 search / list / info / summary。
- **Desktop/Electron App 适配**：`strategy: ui` + `domain: localhost` 时走 CDP 直连模式（`crates/opencli-rs-browser/src/electron_apps.rs`），需目标 App 开启 `--remote-debugging-port`。内置端口映射：antigravity=9234, cursor=9226, codex=9222, chatwise=9228。用户可通过 `~/.opencli-rs/apps.yaml` 扩展。
- **Daemon Socket API**：JSON-RPC over TCP（127.0.0.1:10008），支持 daemon.*、job.*、adapter.* 方法。tools.* 已移除（本地直接执行）。
- **Scheduler 注意事项**：args 为 null 时正常处理；执行前自动注入 YAML 中定义的 default 参数；job ID 支持前缀匹配。

0409
- **原生命令扩展**：新增 `opencli update` / `opencli update --check`，支持检查 GitHub Release 并原地更新当前二进制。
- **反馈命令**：新增 `opencli feedback <title>`，默认写入 `~/.opencli-rs/feedback.jsonl`，加 `--open` 可打开预填好的 GitHub issue 页面。
- **帮助输出收敛**：`opencli --help` 默认只显示内置命令和 daemon/client 命令.

0410
- **版本来源收口**：`opencli update` 与发布安装链路不再依赖 GitHub API 的 `tag_name` 判定版本，统一改为读取 release 固定路径 `releases/latest/download/latest` 的纯文本版本文件。
- **下载入口固定化**：自更新与 `install.sh` 统一走 `releases/latest/download/<asset>` 固定路径，避免版本判断与资产下载来自不同来源。
- **版本号重置起点**：工作区版本已改回 `0.0.1`，为重新整理 tag/release 序列做准备。
- **知乎收藏夹 API 提速**：`zhihu collection_items_api` 改为使用 `bg_fetch` 直接在扩展后台发起请求并注入知乎 cookies，去掉 `navigate + 页面内 fetch`，避免知乎首页导航常见的 15 秒超时拖慢整条命令。
- **只读接口批量提速**：`zhihu search` 改为 `bg_fetch` 后台取数，避免为借 cookie 先开页面导致的额外导航等待；`zhihu feed_api` 与 `weibo hot` 实测存在 payload / 权限边界，暂不切换。

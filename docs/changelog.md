0403 
- **External CLI 执行代理已移除**：opencli-rs-cli 不再依赖 opencli-rs-external，external CLI 透传功能已删除。
- **工具知识库**：本地纯文件方案，从 `~/.opencli-rs/tools/*.md`（YAML frontmatter + Markdown body）加载，内存过滤，无 SQLite、无 daemon 依赖。`opencli-cli tools` 子命令直接本地执行，支持 search / list / info / summary。
- **Desktop/Electron App 适配**：`strategy: ui` + `domain: localhost` 时走 CDP 直连模式（`crates/opencli-rs-browser/src/electron_apps.rs`），需目标 App 开启 `--remote-debugging-port`。内置端口映射：antigravity=9234, cursor=9226, codex=9222, chatwise=9228。用户可通过 `~/.opencli-rs/apps.yaml` 扩展。
- **Daemon Socket API**：JSON-RPC over TCP（127.0.0.1:10008），支持 daemon.*、job.*、adapter.* 方法。tools.* 已移除（本地直接执行）。
- **Scheduler 注意事项**：args 为 null 时正常处理；执行前自动注入 YAML 中定义的 default 参数；job ID 支持前缀匹配。

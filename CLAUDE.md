# CLAUDE.md

## Agents
- 使用中文，当有问题解决不了的时候及时停下反思.
- 新增或修改 adapter 一律在 `/Users/zzwy/tmp/opencli-adapters` 进行：先通过 chrome cdp 调试，再通过本仓库构建的 `opencli` 以本地插件方式验证；本仓库不保存站点 YAML
- 保持项目简洁，并将获取cli这项任务完成到极致.

## 项目概述
opencli-rs 用于从任意网站抓取信息,通过 浏览器插件实现登陆状态复用,yaml adapters 实现扩展。目前有两种模式: 
- cli 模式, 每次执行都经历完整流程：启动浏览器、加载适配器、执行 pipeline、输出结果、退出。每次都会新建浏览器连接。
- daemon 模式 docs/daemon.md ，常驻进程，Socket API 接收命令，浏览器连接复用，支持 adapter/plugin 管理与扩展 action API。

## 架构

- `browser-daemon` 这一层只负责 CDP / WebSocket 消息转发和并发路由，不承担业务级状态机
- 并发是按 `cmd.id` 进行请求-响应配对：daemon 用 `pending_commands` 保存 `id -> oneshot sender`，extension 回包时带回同一个 `id`

## 架构

1. ./crates/opencli-rs-core, CliCommand、Strategy、IPage、CliError、Registry, docs/01-core.md
2. ./crates/opencli-rs-pipeline, StepRegistry、Step、模板系统, docs/02-pipeline.md
3. ./crates/opencli-rs-browser, BrowserBridge、Daemon、DaemonPage、CdpPage、Extension, docs/03-browser.md
4. ./crates/opencli-rs-output, Table、JSON、YAML、CSV、Markdown 渲染, docs/05-output.md
5. ./crates/opencli-rs-discovery, YAML解析、缓存机制, docs/04-discovery.md
6. ./crates/opencli-rs-cli, CLI入口、执行流程、参数处理, docs/08-cli.md
7. ./crates/opencli-rs-daemon, AdapterManager、PluginManager、Socket API
8. adapters/  # YAML 适配器定义（运行时加载）— **产品核心**
9. extension/  # chrome extension

## 额外功能

Adapter 发现: `adapter search`（子串，见 docs/search.md）。本地 KV: `opencli kv --help`。
已移除：tools/summary/feedback/job/history/sync/hidden/FTS/find/socket-exec（专注 adapter 质量）。

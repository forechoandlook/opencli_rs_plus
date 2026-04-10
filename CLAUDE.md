# CLAUDE.md

## Agents

- 使用中文，当有问题解决不了的时候及时停下反思.
- 开发新的adapters的时候 加载 playwright cli skills 尽可能使用api的方式，需要先通过playwright调试，最后固化为adapters 开发测试通过 `cargo run --` 实现, 参考 docs/develop.md
- 功能修改等需要记录到 docs/changelog.md 中
- 保持项目简洁，并将获取cli这项任务完成到极致.

## 项目概述

opencli-rs 用于从任意网站抓取信息,通过 浏览器插件实现登陆状态复用,yaml adapters 实现扩展。目前有两种模式: 
- cli 模式, 每次执行都经历完整流程：启动浏览器、加载适配器、执行 pipeline、输出结果、退出。每次都会新建浏览器连接。
- daemon 模式 docs/daemon.md ，常驻进程，Socket API 接收命令，浏览器连接复用，支持定时任务和 adapter 管理。

## 架构

1. ./crates/opencli-rs-core, CliCommand、Strategy、IPage、CliError、Registry, docs/01-core.md
2. ./crates/opencli-rs-pipeline, StepRegistry、14种Step、模板系统, docs/02-pipeline.md
3. ./crates/opencli-rs-browser, BrowserBridge、Daemon、DaemonPage、CdpPage、Extension, docs/03-browser.md
4. ./crates/opencli-rs-output, Table、JSON、YAML、CSV、Markdown 渲染, docs/05-output.md
5. ./crates/opencli-rs-discovery, YAML解析、缓存机制, docs/04-discovery.md
6. ./crates/opencli-rs-external, 工具知识库资源（external-clis.yaml），不再做执行代理
7. ./crates/opencli-rs-ai, Explore、Cascade、Synthesize、Generate, docs/07-ai.md
8. ./crates/opencli-rs-cli, CLI入口、执行流程、参数处理, docs/08-cli.md
9. ./crates/opencli-rs-daemon, Scheduler、JobStore、AdapterManager、Socket API
10. adapters/  # YAML 适配器定义（运行时加载）
11. extension/  # chrome extension

## 额外功能

Adapter/tools 检索机制: docs/search.md
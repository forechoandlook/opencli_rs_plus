# opencli-rs

`opencli-rs` 是一个面向网站信息获取的命令行工具。它通过浏览器插件复用登录态，用 YAML adapter 描述抓取流程，既可以直接执行单个 adapter，也可以通过 daemon 常驻管理任务、插件和调度。

## 特性

- `daemon` 常驻模式，支持任务调度、adapter 管理和 SQLite 持久化
- `direct` 直接执行模式，适合单次抓取和调试
- YAML adapter 定义，便于新增站点和命令
- 浏览器插件复用登录态，适合需要已登录页面的抓取场景
- 本地工具知识库 `opencli tools`，唯一入口，所有工具都通过 `~/.opencli-rs/tools/*.md` 管理
- 本地反馈记录 `opencli feedback`
- 默认输出格式为 CSV，可通过 `--format` 切换为 `table` / `json` / `yaml` / `md`

## 安装

```bash
curl -fsSL https://github.com/forechoandlook/opencli_rs_plus/releases/latest/download/install.sh | bash
```

默认安装到：

```bash
~/.local/bin/opencli
```

检查安装：

```bash
opencli --version
opencli doctor
```

## 快速开始

查看帮助：

```bash
opencli --help
opencli <family> --help
opencli --format json zhihu hot
```

启动 daemon：

```bash
opencli daemon
opencli status
```

直接执行一个 adapter：

```bash
opencli zhihu hot
opencli bilibili hot
```

查看 adapter：

```bash
opencli adapter list
opencli adapter search "zhihu"
opencli adapter disable "zhihu hot"
opencli adapter enable "zhihu hot"
```

查看本地工具知识库：

```bash
opencli tools search curl
opencli tools list
opencli tools info ripgrep
opencli tools summary
```

记录反馈：

```bash
opencli feedback "zhihu hot returns 403" --adapter "zhihu hot" --kind broken
opencli feedback "adapter summary inaccurate" --kind bad_description --open
```

## 模式说明

`opencli` 的常见运行方式分三类：

- `daemon` 模式：`opencli daemon` 启动调度 daemon，负责任务、adapter 和插件
- `client` 模式：`status` / `stop` / `restart` / `job` / `adapter` / `plugin` / `tools` 等命令连接 daemon
- `direct` 模式：其他 adapter 命令直接执行，不依赖 daemon

帮助输出默认不会直接展开全部 adapter，避免过长；要看某个 adapter family，请直接使用 `opencli <family> --help`。

## 反馈

`opencli feedback` 用于记录用户问题、适配器异常和文档不准确，默认写入本地：

```bash
~/.opencli-rs/feedback.jsonl
```

它支持：

- `--body` / `-m` 补充说明
- `--adapter` 关联具体 adapter
- `--kind broken|bad_description|other`
- `--open` 打开预填好的 GitHub issue 页面

默认不会打印 adapter 加载过程；如需查看调试级别信息，可结合 `--verbose` 或 `RUST_LOG=debug` 使用。

## 开发

新增或修改 adapter 时，推荐流程是：

1. 先用 Playwright 或浏览器调试把页面流程跑通
2. 将验证过的逻辑固化到 YAML adapter 的 `evaluate` / pipeline 步骤
3. 用 `cargo run -- <site> <command>` 做端到端测试
4. 必要时更新 `docs/changelog.md`

开发细节和 schema 约定见：

- [docs/develop.md](docs/develop.md)
- [docs/daemon.md](docs/daemon.md)
- [docs/search.md](docs/search.md)

## 目录结构

- `crates/opencli-rs-core` 核心抽象与 registry
- `crates/opencli-rs-pipeline` pipeline 和 step 系统
- `crates/opencli-rs-browser` 浏览器桥接、daemon、CDP 相关实现
- `crates/opencli-rs-output` 表格、JSON、YAML、CSV、Markdown 输出
- `crates/opencli-rs-discovery` YAML 解析和缓存
- `crates/opencli-rs-external` 工具知识库资源
- `crates/opencli-rs-ai` Explore / Cascade / Synthesize / Generate
- `crates/opencli-rs-cli` CLI 入口与执行流程
- `crates/opencli-rs-daemon` scheduler、JobStore、Socket API、AdapterManager
- `adapters/` 运行时加载的 YAML adapter
- `extension/` Chrome extension

## 版本与发布

当前 workspace 版本来自 `Cargo.toml` 的 `workspace.package.version`。发布和自更新相关逻辑以仓库内的 release / install 机制为准，建议在修改命令行为后同步检查：

```bash
cargo test
opencli --version
opencli update --check
```

## 文档

- [docs/daemon.md](docs/daemon.md)
- [docs/develop.md](docs/develop.md)
- [docs/search.md](docs/search.md)
- [docs/changelog.md](docs/changelog.md)

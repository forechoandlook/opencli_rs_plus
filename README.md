# opencli-rs

`opencli-rs` 是一个面向网站信息获取的命令行工具。它通过浏览器插件复用登录态，用 YAML adapter 描述抓取流程，既可以直接执行单个 adapter，也可以通过 daemon 常驻管理 adapter 与插件。

## 特性

- **YAML adapter** 为核心：把网站变成可组合的 CLI 命令
- `direct` 直接执行模式，适合单次抓取和调试
- `daemon` 常驻：adapter/plugin 管理、扩展当前页 action、检索索引
- 浏览器插件复用登录态
- 本地 KV 缓存稳定身份字段（如 `bilibili:me.mid`）
- 官方 adapter 插件：`opencli plugin install forechoandlook/opencli-adapters`
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

本地开发构建并安装 release 二进制：

```bash
./scripts/build-release.sh
```

脚本会运行 `cargo build --release --bin opencli`，然后覆盖安装到 `~/.local/bin/opencli`。

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

发现 adapter 用法：

```bash
opencli adapter search zhihu
opencli plugin install forechoandlook/opencli-adapters
opencli kv list
```

## 模式说明

`opencli` 的常见运行方式分三类：

- `daemon` 模式：`opencli daemon` 负责 adapter/plugin 管理与扩展 API
- `client` 模式：`daemon *` / `adapter` / `plugin` / `kv`
- `direct` 模式：其他 adapter 命令直接执行，不依赖 daemon

帮助输出默认不会直接展开全部 adapter，避免过长；要看某个 adapter family，请直接使用 `opencli <family> --help`。

默认不会打印 adapter 加载过程；如需调试信息，用 `--verbose` 或 `RUST_LOG=debug`。

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
- `crates/opencli-rs-cli` CLI 入口与执行流程
- `crates/opencli-rs-daemon` AdapterManager、PluginManager、Socket API
- `adapters/` 运行时加载的 YAML adapter（产品核心）
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

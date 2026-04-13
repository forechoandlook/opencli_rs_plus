---
name: opencli
description: opencli 的安装、启动、daemon/client 模式、adapter 发现、工具检索、发布验证或功能实现时触发. 获取网站信息的命令行工具，支持浏览器交互和 YAML 定义的适配器.
author: "zwy"
version: "0.1"
---

## 检查,安装,更新
是否安装: `opencli --version`
安装: `curl -fsSL https://github.com/forechoandlook/opencli_rs_plus/releases/latest/download/install.sh | bash`
安装后默认路径：`~/.local/bin/opencli`.
更新：`opencli update` 或重新执行安装脚本.
`opencli doctor` 进行环境检查和问题排查.

## 模式判断

`opencli` 有三类常见路径：

- `daemon` 模式：`opencli daemon` 启动调度 daemon，负责任务调度、adapter 管理、SQLite 持久化和 socket API。
- `client` 模式：`status` / `stop` / `restart` / `job` / `adapter` / `plugin` / `socket` / `tools` 等命令会连接 daemon。
- `direct` 模式：其他 adapter 命令直接执行，不依赖 daemon；daemon 未运行时也可用于调试单个 adapter。

- `opencli --help` 默认只展示内置命令和 daemon/client 命令。
- `opencli <family> --help` 查看某个 family。

## 常用命令

`--fields` 选项适用于支持的 adapter，指定输出字段（逗号分隔，保持顺序）

```bash
opencli --help
opencli --help --adapters
opencli daemon
opencli status
opencli restart
opencli adapter list
opencli tool list
opencli adapter search "zhihu"
opencli plugin list
opencli tools search curl
opencli tools info ripgrep
```

## daemon 约定

- 调度 daemon 默认监听 `127.0.0.1:10008`。
- browser-daemon 负责和 Chrome 插件保持长连接，端口范围默认 `19825-19834`。
- 需要浏览器的 adapter 会由 daemon 通过 browser-daemon 转发到 Chrome 插件执行。
- 安装、卸载、更新 plugin 后，daemon 会自动重新加载 adapters。

## adapter 与插件

- adapter 是运行时加载的 YAML 定义，插件可以来自 GitHub 仓库、本地路径或子目录。
- `opencli adapter search` 用于按名称、描述或使用热点查找 adapter。
- `opencli adapter disable/enable` 是持久化控制，避免无效命令继续出现在帮助或检索里。
- 如果用户在找某个外部工具信息，先用 `opencli tools search/list/info/summary`，这些是本地文件，不依赖 daemon。

## 常见排查

```bash
opencli status
opencli feedback "zhihu hot returns 403" --adapter "zhihu hot" --kind broken
```

## 问题反馈

`opencli feedback` 用于把用户遇到的问题、错误结果、文档不准确之处记录到本地，并可选打开预填好的 GitHub issue 页面。 参考 [反馈文档](./feedback.md) 获取详细用法和上报建议。
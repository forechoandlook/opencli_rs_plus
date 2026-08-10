## Daemon 模式

所有功能集成在单一 `opencli` 二进制中，内部由两个独立进程组成：

- **opencli daemon**（`opencli daemon`）— adapter/plugin 管理、扩展 action API、TCP Socket API（默认端口 10008）
- **browser-daemon**（`opencli-rs-browser` crate 内）— 管理与 Chrome 插件的 WebSocket 长连接，代理 CDP 命令，监听端口 19825-19834

`opencli` 命令路由规则：

| 第一个参数 | 行为 |
|---|---|
| `daemon` | 启动 opencli daemon |
| `daemon start/stop/status/...` | 进程管理 |
| `adapter` / `plugin` / `kv` | 管理命令（部分可不连 daemon） |
| 其他（如 `zhihu hot`）| 直接执行 adapter |

需要浏览器的 adapter 在 direct 模式下经 browser-daemon 转发到 Chrome 插件执行。

### 浏览器插件当前页面操作

daemon 还会在 `127.0.0.1:10009` 启动仅供 Chrome 扩展调用的本地 action-discovery API。扩展打开时读取当前标签 URL，向 daemon 请求已注册且匹配该 URL 的 action；点击后的读取与下载在用户当前标签内直接完成，browser-daemon 不参与这类操作。

adapter 通过可选的 `context` 字段注册操作：

```yaml
context:
  title: 下载当前笔记
  paths: ["/explore/*"]
  activeTab:
    usePipeline: true
  args:
    note-id: current_url
```

daemon 仅负责 action 发现，扩展会解释 YAML 的 `activeTab` 计划：`usePipeline` 复用 adapter 既有 `evaluate`/`limit`/`download` 步骤，但跳过 `navigate`；`extract` 则用于直接读取当前页面的简短提取器。扩展只读取用户已打开页面的 DOM/状态并通过浏览器下载 API 落盘，不会导航、滚动、点击、创建自动化窗口或写入任务面板。adapter 需要显式写出 `activeTab` 才能在当前页运行；daemon 默认会校验 adapter `domain` 与 `paths`，当页面使用另一明确子域时可用 `context.hosts` 声明允许的主机名；API 只接受 `chrome-extension://` Origin，网页不能跨域调用。扩展和 daemon 默认通过 `127.0.0.1:10009` 通信。

新增当前页面能力时，先在登录态页面验证只读提取逻辑，再优先用 `usePipeline: true` 复用已有 adapter YAML；只有原 pipeline 包含 `tap`、`fetch` 等非当前页步骤时才在 `activeTab.extract` 增加短提取器。当前页执行会运行该 adapter 的 YAML `evaluate`，所以只对用户已启用的、受信任 adapter 开放；扩展不会接受网页传入的脚本。

排查时用 `OPENCLI_VERBOSE=1 cargo run -- daemon` 启动调度 daemon；它会打印当前页面 action 查询的 host、path 与匹配数，但不会记录 URL query（避免记录短期签名）。弹窗也会显示匹配数和 API 端口，并将相同的脱敏诊断转发到 browser-daemon 的 extension log。

### 命令发现

顶层帮助默认只展示内置命令和 daemon/client 命令，不直接展开全部 adapter，避免输出过长：

```bash
opencli --help
```

查看某个 adapter family 下的具体命令：

```bash
opencli zhihu --help
opencli zhihu hot --help
```

```
opencli status / adapter / plugin / ...
    ↓ TCP JSON-RPC (127.0.0.1:10008)
opencli daemon
    ├── AdapterManager  (加载 / 子串搜索 / enable-disable)
    └── PluginManager   (插件安装/卸载/更新，plugins.lock.json)
         （扩展 action API 127.0.0.1:10009）

opencli <site> <cmd>  (direct)
    ↓ HTTP POST /command (需要浏览器时)
browser-daemon (127.0.0.1:19825-19834)
    ↓ WebSocket /ext
Chrome 插件 → CDP
```

### 启动和管理

```bash
# 前台启动（阻塞终端）
opencli daemon
opencli daemon --addr 0.0.0.0:10008   # 自定义地址

# 后台启动/停止/重启（自动 detach，日志写入 ~/.opencli-rs/daemon.log）
opencli daemon start
opencli daemon stop
opencli daemon restart

# 查看状态 / 日志 / 配置
opencli daemon status        # 等价于 opencli status
opencli daemon logs -f
opencli daemon logs -n 100
opencli daemon config

# 开机自启动（macOS launchd / Linux systemd --user）
opencli daemon autostart install
opencli daemon autostart uninstall
opencli daemon autostart status

# 直接执行 adapter（无需 daemon）
opencli zhihu hot
opencli bilibili hot

# 搜索 adapter（子串 + 用法）
opencli adapter search zhihu

opencli --help
```

定时调度（`job`）已移除；需要定时执行时用系统 `cron` / `launchd` 调用 `opencli <site> <cmd>`。

### Adapter 管理

```bash
# 查看所有 adapters（隐藏已禁用的）
opencli adapter list

# 搜索 adapters（子串匹配，打印用法）
opencli adapter search "zhihu"

# 禁用/启用 adapter（持久化；help/search/direct 执行都会生效，可不依赖 daemon）
opencli adapter disable "zhihu hot"
opencli adapter disable zhihu/hot
opencli adapter disable wikipedia      # 整站
opencli adapter enable "zhihu hot"
opencli adapter list --include-disabled

```

### Adapter 搜索

```bash
opencli adapter search collection_items
```

子串匹配；每条命中打印 description 与可复制用法。见 [search.md](search.md)。


### Socket API（调试用）

```bash
### Plugin 管理

插件是包含 YAML adapter 文件的目录，可通过 GitHub 仓库、本地路径安装。安装后 adapter 立即生效，无需重启 daemon。

**安装来源格式：**

| 格式 | 说明 |
|---|---|
| `github:user/repo` | 从 GitHub 克隆整个仓库 |
| `github:user/repo/subpath` | 克隆仓库，只安装 `subpath/` 子目录作为插件 |
| `https://github.com/user/repo.git` | 完整 HTTPS URL |
| `git@github.com:user/repo.git` | SSH URL |
| `file:///absolute/path` | 本地目录（符号链接，改动实时生效）|
| `local:/path` | 同上 |
| `/absolute/path` | 同上 |

**插件 manifest（`opencli-plugin.json`，可选）：**

```json
{
  "name": "my-plugin",
  "version": "0.1.0",
  "description": "My custom adapters",
  "opencli": ">=0.1.0"
}
```

若无 manifest，插件名取自目录名，目录内所有 `.yaml` 文件作为 adapter 加载。

```bash
# 官方 adapter 包（推荐用户安装路径）
opencli plugin install forechoandlook/opencli-adapters
opencli plugin update opencli-adapters

# 安装插件（整个仓库，裸 user/repo 自动补 github:）
opencli plugin install user/my-plugin
# 安装仓库中的某个子目录
opencli plugin install user/monorepo/plugins/my-plugin
# 本地目录（符号链接，开发用）
opencli plugin install /path/to/local-plugin

# 查看已安装插件
opencli plugin list

# 更新指定插件（git pull 或重新克隆）
opencli plugin update my-plugin

# 更新所有插件
opencli plugin update

# 卸载插件
opencli plugin uninstall my-plugin
```

开发仍以本仓库 `adapters/` 为准；向用户分发时用 `scripts/sync-adapters-repo.sh` 同步到 [opencli-adapters](https://github.com/forechoandlook/opencli-adapters) 并打 tag。默认不自动静默覆盖已装插件。

安装/卸载/更新后 daemon 自动重新加载所有 adapter（等同于 `adapter.reload`）。

**插件存储位置：**

```
~/.opencli-rs/plugins/
    my-plugin/           ← git 克隆或本地符号链接
        opencli-plugin.json
        search.yaml
        trending.yaml
    another-plugin/
        ...
~/.opencli-rs/plugins.lock.json   ← 记录安装来源和时间
```

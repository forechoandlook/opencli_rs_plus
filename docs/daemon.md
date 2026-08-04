## Daemon 调度模式

所有功能集成在单一 `opencli` 二进制中，内部由两个独立进程组成：

- **调度 daemon**（`opencli daemon`）— 任务调度、adapter 管理、SQLite 持久化、TCP Socket API（默认端口 10008）
- **browser-daemon**（浏览器 daemon，`opencli-rs-browser` crate 内）— 管理与 Chrome 插件的 WebSocket 长连接，代理 CDP 命令，监听端口 19825-19834

`opencli` 命令路由规则：

| 第一个参数 | 行为 |
|---|---|
| `daemon` | 启动调度 daemon |
| `status` / `stop` / `restart` / `job` / `adapter` / `plugin` / `tools` | 作为调度客户端连接 daemon |
| 其他（如 `zhihu hot`）| 直接执行 adapter |

调度 daemon 在执行需要浏览器的 adapter 时，通过 HTTP POST 调用 browser-daemon，browser-daemon 再通过 WebSocket 转发给 Chrome 插件执行。

### 浏览器插件当前页面操作

调度 daemon 还会在 `127.0.0.1:10009` 启动一个仅供 Chrome 扩展调用的本地 action-discovery API。扩展打开时读取当前标签 URL，向 daemon 请求已注册且匹配该 URL 的 action；点击后的读取与下载在用户当前标签内直接完成，browser-daemon 不参与这类操作。

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

daemon 仅负责 action 发现，扩展会解释 YAML 的 `activeTab` 计划：`usePipeline` 复用 adapter 既有 `evaluate`/`limit`/`download` 步骤，但跳过 `navigate`；`extract` 则用于直接读取当前页面的简短提取器。扩展只读取用户已打开页面的 DOM/状态并通过浏览器下载 API 落盘，不会导航、滚动、点击、创建自动化窗口或写入任务面板。adapter 需要显式写出 `activeTab` 才能在当前页运行；daemon 仍会校验 `domain` 与 `paths`，API 只接受 `chrome-extension://` Origin；网页不能跨域调用。扩展和 daemon 默认通过 `127.0.0.1:10009` 通信。

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
opencli status / job / adapter / ...
    ↓ TCP JSON-RPC (127.0.0.1:10008)
opencli daemon
    ├── AdapterManager  (adapter 加载/管理/搜索/禁用)
    ├── AdapterIndex    (FTS5 全文索引 + 使用统计，index.db)
    ├── PluginManager   (插件安装/卸载/更新，plugins.lock.json)
    ├── Scheduler       (轮询 due jobs，并发执行)
    └── JobStore        (SQLite CRUD，指数退避重试，jobs.db)
         ↓ HTTP POST /command (需要浏览器时)
    browser-daemon (127.0.0.1:19825-19834)
         ↓ WebSocket /ext
    Chrome 插件
         ↓ CDP
    Chrome 页面（并发执行，最后一个任务完成后 120s 关闭窗口）
```

### 启动和管理

```bash
# 启动 daemon（阻塞前台，建议配合 nohup 或 systemd 后台运行）
opencli daemon
opencli daemon --poll-interval 10
opencli daemon --addr 0.0.0.0:10008   # 自定义地址

# 后台运行示例（macOS/Linux）
nohup opencli daemon > ~/.opencli-rs/daemon.log 2>&1 &

# 查看状态
opencli status

# 停止/重启
opencli stop
opencli restart

# 连接远程 daemon
opencli --addr 192.168.1.100:10008 status

# 直接执行 adapter（无需 daemon）
opencli zhihu hot
opencli bilibili hot

# 查看顶层命令
opencli --help
```

### Adapter 管理

```bash
# 查看所有 adapters（隐藏已禁用的）
opencli adapter list

# 搜索 adapters（FTS5/BM25 全文检索 + 使用热点混合排序）
opencli adapter search "zhihu"

# 禁用/启用 adapter（持久化，不显示在 help 中）
opencli adapter disable "zhihu hot"
opencli adapter enable "zhihu hot"

# 同步 adapters（从指定目录增量更新索引）
opencli adapter sync --folder /path/to/adapters
```

### Job 管理

```bash
# 添加任务
opencli job add "zhihu hot" --delay 300              # 5分钟后执行
opencli job add "bilibili hot" --interval 3600       # 每小时循环
opencli job add "zhihu collection_items_api" --args '{"collection_id":"123"}' --run-at "2026-03-31T10:00:00Z"

# 查看任务
opencli job list --status pending
opencli job show <id>

# 取消/删除
opencli job cancel <id>
opencli job delete <id>

# 手动触发 due jobs
opencli job run
```

### 工具知识库

工具知识库是唯一的 `opencli tools` 入口，用来索引 CLI 工具信息（名称、描述、安装命令等），所有工具都通过 `~/.opencli-rs/tools/*.md` 管理。

**本地纯文件方案，不经过 daemon**，直接读取 `~/.opencli-rs/tools/*.md`，内存过滤。

每个 `.md` 文件格式：

```markdown
---
name: ripgrep
binary: rg
homepage: https://github.com/BurntSushi/ripgrep
tags: [search, grep, regex]
install:
  mac: brew install ripgrep
  linux: apt install ripgrep
---

Fast line-oriented regex search tool.（第一行 = short description）

更详细的说明...
```

```bash
opencli tools search <query>    # 关键词搜索（名称、binary、描述、标签）
opencli tools list              # 列出所有工具
opencli tools info <name>       # 查看工具详情（含完整 markdown body）
opencli tools summary           # 所有工具名称 + 短描述 + 是否已安装
```

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

## Daemon 调度模式

所有功能集成在单一 `opencli` 二进制中，内部由两个独立进程组成：

- **调度 daemon**（`opencli daemon`）— 任务调度、adapter 管理、SQLite 持久化、TCP Socket API（默认端口 10008）
- **browser-daemon**（浏览器 daemon，`opencli-rs-browser` crate 内）— 管理与 Chrome 插件的 WebSocket 长连接，代理 CDP 命令，监听端口 19825-19834

`opencli` 命令路由规则：

| 第一个参数 | 行为 |
|---|---|
| `daemon` | 启动调度 daemon |
| `status` / `stop` / `restart` / `job` / `adapter` / `plugin` / `socket` / `tools` | 作为调度客户端连接 daemon |
| 其他（如 `zhihu hot`）| 直接执行 adapter |

调度 daemon 在执行需要浏览器的 adapter 时，通过 HTTP POST 调用 browser-daemon，browser-daemon 再通过 WebSocket 转发给 Chrome 插件执行。

```
opencli status / job / adapter / ...
    ↓ TCP JSON-RPC (127.0.0.1:10008)
opencli daemon
    ├── AdapterManager  (adapter 加载/管理/搜索/禁用)
    ├── AdapterIndex    (FTS5 全文索引 + 使用统计，index.db)
    ├── PluginManager   (插件安装/卸载/更新，plugins.lock.json)
    ├── IssueStore      (问题记录，issues.db)
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
```

### Adapter 管理

```bash
# 查看所有 adapters（隐藏已禁用的）
opencli adapter list

# 搜索 adapters（FTS5/BM25 全文检索 + 使用热点混合排序）
opencli adapter search "zhihu"

# 查看使用热点（按累计调用次数排序，默认 top 20）
opencli socket adapter.hot '{"limit":10}'

# 查看最近 7 天活跃 adapters
opencli socket adapter.trending '{"days":7,"limit":10}'

# 禁用/启用 adapter（持久化，不显示在 help 中）
opencli adapter disable "zhihu hot"
opencli adapter enable "zhihu hot"

# 同步 adapters（从指定目录增量更新索引）
opencli adapter sync --folder /path/to/adapters

# 重新加载所有 adapters 并增量同步索引
opencli socket adapter.reload

# 强制全量重建 FTS 索引（索引损坏时使用）
opencli socket adapter.reindex
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

### Issue 管理

记录 adapter 存在的问题，支持后续导出和上报。

```bash
# 上报问题（kind: broken | bad_description | other）
opencli socket issue.add '{"adapter":"bilibili feed","kind":"broken","title":"API v3 变更，返回 404","body":"2026-04 起接口地址改变"}'

# 查看所有 open 问题
opencli socket issue.list

# 按 adapter 过滤
opencli socket issue.list '{"adapter":"bilibili feed"}'

# 查看已关闭的问题
opencli socket issue.list '{"status":"closed"}'

# 查看某条问题详情
opencli socket issue.show '{"id":1}'

# 关闭问题（已修复）
opencli socket issue.close '{"id":1}'

# 删除问题
opencli socket issue.delete '{"id":1}'

# 导出为 JSON
opencli socket issue.export
opencli socket issue.export '{"status":"open"}' > issues.json
```

**issue kind：**

| kind | 含义 |
|---|---|
| `broken` | 工具损坏，API 变更、返回错误或错误结果 |
| `bad_description` | summary / description 文本不准确 |
| `other` | 其他问题 |

### 工具知识库

工具知识库索引 CLI 工具信息（名称、描述、安装命令等），用于帮助 agent 发现不熟悉的工具。

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
# 手动发送 socket 请求
opencli socket daemon.status
opencli socket adapter.search '{"query":"bilibili"}'
```

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

### 数据文件

| 文件 | 说明 |
|---|---|
| `~/.opencli-rs/jobs.db` | 调度任务（status / retry / interval） |
| `~/.opencli-rs/index.db` | FTS5 全文索引 + 使用统计 + 索引元数据（mtime） |
| `~/.opencli-rs/issues.db` | Adapter 问题记录 |
| `~/.opencli-rs/adapter_settings.json` | adapter 启用/禁用/隐藏状态 |
| `~/.opencli-rs/plugins/` | 已安装插件目录 |
| `~/.opencli-rs/plugins.lock.json` | 插件安装来源和时间记录 |
| `~/.opencli-rs/tools/*.md` | 工具知识库（本地文件，不需要 daemon） |

### Adapter 检索机制

**搜索排序：**
```
score = 0.7 × BM25(query, adapter) + 0.3 × log(1 + usage_count)
```
- BM25 对 `full_name`、`site`、`name`、`description`、`domain`、`summary` 做全文匹配
- `usage_count` 每次 `exec` 成功后自动累计，热点 adapter 在同等匹配度下优先排列
- `disabled` 的 adapter 在 FTS 检索后、返回前被过滤掉

**索引更新策略（增量 sync）：**

daemon 启动、`adapter.reload`、`adapter.sync` 时触发增量同步：

| 情况 | 处理 |
|---|---|
| 新增 adapter | INSERT into FTS，写入 meta |
| yaml 或 summary.md mtime 变化 | 删除旧行，重新 INSERT |
| adapter 已删除 | 从 FTS 和 meta 中删除 |
| 无变化 | 跳过 |

`adapter_index_meta` 表记录每个 adapter 的 yaml mtime + summary mtime，是增量判断的依据。

**Socket API：**

| 方法 | 参数 | 说明 |
|---|---|---|
| `adapter.search` | `query`, `include_hidden` | BM25 + 热点混合搜索 |
| `adapter.hot` | `limit`（默认 20）| 按累计使用次数排序 |
| `adapter.trending` | `days`（默认 7）, `limit` | 最近 N 天内活跃的 adapters |
| `adapter.reload` | — | 重新扫描 yaml 文件并增量同步索引 |
| `adapter.reindex` | — | 强制全量重建 FTS 索引 |
| `adapter.sync` | `folder` | 从指定目录同步并增量更新索引 |
| `plugin.install` | `path` | 安装插件并重载 adapter |
| `plugin.uninstall` | `name` | 卸载插件并重载 adapter |
| `plugin.list` | — | 列出所有已安装插件 |
| `plugin.update` | `name`（可选，不传则更新全部）| 更新插件（git pull）并重载 adapter |

## Daemon 调度模式

daemon 模式由两个独立进程组成：

- **`opencli-rs-daemon`**（调度 daemon）— 任务调度、adapter 管理、SQLite 持久化、TCP Socket API（默认端口 10008）
- **`browser-daemon`**（浏览器 daemon，`opencli-rs-browser` crate 内）— 管理与 Chrome 插件的 WebSocket 长连接，代理 CDP 命令，监听端口 19825-19834

两者关系：`opencli-rs-daemon` 在执行需要浏览器的 adapter 时，通过 HTTP POST 调用 `browser-daemon`，`browser-daemon` 再通过 WebSocket 转发给 Chrome 插件执行。

```
Client (CLI/TCP)
    ↓ TCP JSON-RPC (127.0.0.1:10008)
opencli-rs-daemon
    ├── AdapterManager  (adapter 加载/管理/搜索/禁用)
    ├── AdapterIndex    (FTS5 全文索引 + 使用统计，index.db)
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
# 启动 daemon（后台运行，默认监听 127.0.0.1:10008）
opencli-daemon --poll-interval 10

# 自定义地址（支持跨机器访问）
opencli-daemon --addr 0.0.0.0:10008

# 查看状态（通过 cli 连接 daemon）
opencli-cli status

# 停止/重启
opencli-cli stop
opencli-cli restart

# 连接远程 daemon
opencli-cli --addr 192.168.1.100:10008 status
```

### Adapter 管理

```bash
# 查看所有 adapters（隐藏已禁用的）
opencli-cli adapter list

# 搜索 adapters（FTS5/BM25 全文检索 + 使用热点混合排序）
opencli-cli adapter search "zhihu"

# 查看使用热点（按累计调用次数排序，默认 top 20）
opencli-cli socket adapter.hot '{"limit":10}'

# 查看最近 7 天活跃 adapters
opencli-cli socket adapter.trending '{"days":7,"limit":10}'

# 禁用/启用 adapter（持久化，不显示在 help 中）
opencli-cli adapter disable "zhihu hot"
opencli-cli adapter enable "zhihu hot"

# 同步 adapters（从指定目录增量更新索引）
opencli-cli adapter sync --folder /path/to/adapters

# 重新加载所有 adapters 并增量同步索引
opencli-cli socket adapter.reload

# 强制全量重建 FTS 索引（索引损坏时使用）
opencli-cli socket adapter.reindex
```

### Job 管理

```bash
# 添加任务
opencli-cli job add "zhihu hot" --delay 300              # 5分钟后执行
opencli-cli job add "bilibili hot" --interval 3600       # 每小时循环
opencli-cli job add "zhihu collection_items_api" --args '{"collection_id":"123"}' --run-at "2026-03-31T10:00:00Z"

# 查看任务
opencli-cli job list --status pending
opencli-cli job show <id>

# 取消/删除
opencli-cli job cancel <id>
opencli-cli job delete <id>

# 手动触发 due jobs
opencli-cli job run
```

### Issue 管理

记录 adapter 存在的问题，支持后续导出和上报。

```bash
# 上报问题（kind: broken | bad_description | other）
opencli-cli socket issue.add '{"adapter":"bilibili feed","kind":"broken","title":"API v3 变更，返回 404","body":"2026-04 起接口地址改变"}'

# 查看所有 open 问题
opencli-cli socket issue.list

# 按 adapter 过滤
opencli-cli socket issue.list '{"adapter":"bilibili feed"}'

# 查看已关闭的问题
opencli-cli socket issue.list '{"status":"closed"}'

# 查看某条问题详情
opencli-cli socket issue.show '{"id":1}'

# 关闭问题（已修复）
opencli-cli socket issue.close '{"id":1}'

# 删除问题
opencli-cli socket issue.delete '{"id":1}'

# 导出为 JSON
opencli-cli socket issue.export
opencli-cli socket issue.export '{"status":"open"}' > issues.json
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
opencli-cli tools search <query>    # 关键词搜索（名称、binary、描述、标签）
opencli-cli tools list              # 列出所有工具
opencli-cli tools info <name>       # 查看工具详情（含完整 markdown body）
opencli-cli tools summary           # 所有工具名称 + 短描述 + 是否已安装
```

### Socket API（调试用）

```bash
# 手动发送 socket 请求
opencli-cli socket daemon.status
opencli-cli socket adapter.search '{"query":"bilibili"}'
```

### 数据文件

| 文件 | 说明 |
|---|---|
| `~/.opencli-rs/jobs.db` | 调度任务（status / retry / interval） |
| `~/.opencli-rs/index.db` | FTS5 全文索引 + 使用统计 + 索引元数据（mtime） |
| `~/.opencli-rs/issues.db` | Adapter 问题记录 |
| `~/.opencli-rs/adapter_settings.json` | adapter 启用/禁用/隐藏状态 |
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

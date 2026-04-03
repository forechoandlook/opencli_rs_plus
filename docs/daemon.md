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
    ├── Scheduler       (轮询 due jobs，并发执行)
    └── JobStore        (SQLite CRUD，指数退避重试)
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

# 搜索 adapters
opencli-cli adapter search "zhihu"

# 禁用/启用 adapter（持久化，不显示在 help 中）
opencli-cli adapter disable "zhihu hot"
opencli-cli adapter enable "zhihu hot"

# 同步 adapters（替换自动发现，手动指定文件夹）
opencli-cli adapter sync --folder /path/to/adapters
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

### Socket API（调试用）

```bash
# 手动发送 socket 请求
opencli-cli socket daemon.status
opencli-cli socket adapter.search '{"query":"bilibili"}'
```

### 数据文件
- `~/.opencli-rs/jobs.db` — 任务数据库
- `~/.opencli-rs/adapter_settings.json` — adapter 启用/禁用状态 `{"disabled": [], "hidden": []}`
- TCP `127.0.0.1:10008` — 默认监听地址，可通过 `--addr` 覆盖


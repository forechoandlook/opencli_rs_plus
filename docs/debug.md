# 调试指南（最短路径优先）

抓取失败时，按下面的顺序排查，能跳过 daemon 就跳过，链路越短越好定位问题。

## 第 0 层：先分清是哪条链路

```
opencli <site> <cmd>              # 直接执行 adapter，不经过 opencli daemon
    │
    ├─ browser: false             # 纯 HTTP，无浏览器，最简单
    │
    └─ browser: true              # 需要浏览器
         └─ opencli 进程内的 browser-daemon (127.0.0.1:19825-19834)
              └─ WebSocket → Chrome 插件 → CDP → 页面
```

大多数"抓不到数据"的问题出在 browser:true 这条线上（插件没连上/页面选择器错/接口变了），不是 opencli daemon 的问题。**先确认你的 adapter 是不是 `browser: true`，不是的话直接跳到第 3 层。**

## 第 1 层：单命令直连（90% 的问题在这里解决）

```bash
OPENCLI_VERBOSE=1 cargo run -- <site> <cmd>
```

- 不需要启动 `opencli daemon`
- `OPENCLI_VERBOSE=1` 打开日志，能看到 adapter 加载、pipeline 每个 step 的执行情况
- 如果这一步就报错（YAML 解析失败、字段缺失、selector 报错），问题在 adapter YAML 或 pipeline 逻辑本身，不用往下查浏览器/daemon

## 第 2 层：怀疑数据不对/被墙，用 API dump 落盘对比

```bash
OPENCLI_API_DUMP=1 cargo run -- <site> <cmd>
# 结果在 ./data/api-dumps/ 下，对照页面实际返回的结构
```

用来判断是 `evaluate`/`fetch` 拿到的原始数据就不对，还是后面 `map`/`filter`/`select` 处理时丢字段——把问题定位到 pipeline 的哪一个 step。

## 第 3 层：怀疑是浏览器插件没连上 / 页面层面的问题

先确认 opencli Chrome 插件已加载且处于"已连接"状态（扩展图标或 `opencli doctor`），再用 **Playwright CLI 的 Chrome 扩展通道**探索页面。它直接控制用户当前 Chrome Profile，复用现有登录态，不需要给默认 Profile 开启 CDP 端口。

前提：已安装并启用 Playwright MCP Chrome 扩展，并从扩展取得连接 Token。Token 是凭据；开发机可将它写入 `~/.zshrc`，以便新终端自动可用，但绝不能提交到仓库或写入日志。

```bash
# 仅在本机 ~/.zshrc 配置一次；此处必须替换为自己的 Token
export PLAYWRIGHT_MCP_EXTENSION_TOKEN='从 Playwright MCP 扩展取得的 Token'

# 修改 ~/.zshrc 后，在当前终端生效
source ~/.zshrc

# 连接当前已打开的 Chrome；不启动新浏览器，也不需要 --remote-debugging-port
playwright-cli -s=opencli-debug attach --extension=chrome

# 仅探索：打开页面、读取无障碍结构、读取 DOM
playwright-cli -s=opencli-debug tab-new https://www.zhihu.com/
playwright-cli -s=opencli-debug snapshot --depth=6
playwright-cli -s=opencli-debug eval 'document.title'

# 完成后只断开，不关闭用户浏览器
playwright-cli -s=opencli-debug detach
```

用该通道确认选择器、页面结构和只读 JS 逻辑后，再在 [opencli-adapters](https://github.com/forechoandlook/opencli-adapters) 写回 YAML，并用本仓库构建的 `opencli` 以本地插件方式验证。涉及发帖、下单、发送消息、删除等写操作时，先停下并取得用户明确确认。

Chrome 136 起，默认用户数据目录会忽略 `--remote-debugging-port` 和 `--remote-debugging-pipe`；因此不要再把日常 Profile 作为手动 CDP 调试目标。CDP 仅保留给 opencli 插件内部转发，或使用独立的非默认测试 Profile 的特殊场景。见 [Chrome 的官方说明](https://developer.chrome.com/blog/remote-debugging-port?hl=zh-cn)。

## 速查表

| 现象 | 先查哪一层 |
|---|---|
| 命令直接报错/panic | 第1层，看 VERBOSE 日志 |
| 拿到空数据/字段缺失 | 第2层，API dump 对比 |
| 卡住不返回/超时 | 第3层，确认插件连接状态，再通过 Playwright 扩展通道检查页面 |
| 新写的 adapter 选择器不对 | 在 opencli-adapters 中用 Playwright CLI 扩展通道重新探索，不要直接改 YAML 猜 |

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

## 第 3 层：怀疑是浏览器插件没连上 / CDP 层面的问题

先确认 Chrome 插件已加载且处于"已连接"状态（扩展图标/popup 里看连接状态），然后：

```bash
# 绕过 opencli 内置 browser-daemon，直连你手动起的 Chrome CDP 端口
OPENCLI_CDP_ENDPOINT=http://127.0.0.1:9222 cargo run -- <site> <cmd>
```

这一层能排除"browser-daemon 转发出问题"还是"页面/选择器本身有问题"。如果直连 CDP 还是不行，去 chrome-cdp / playwright-cli 里手动跑一遍探索流程（见 docs/develop.md Step 1），确认选择器/JS 逻辑本身是对的，再回来写回 YAML。

## 速查表

| 现象 | 先查哪一层 |
|---|---|
| 命令直接报错/panic | 第1层，看 VERBOSE 日志 |
| 拿到空数据/字段缺失 | 第2层，API dump 对比 |
| 卡住不返回/超时 | 第3层，确认插件连接状态，再直连 CDP |
| 新写的 adapter 选择器不对 | 回 docs/develop.md，用 chrome-cdp/playwright-cli 重新探索，不要直接改 YAML 猜 |

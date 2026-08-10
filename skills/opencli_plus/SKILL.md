---
name: opencli
description: opencli 的安装、daemon、adapter 发现与 disable、KV、写 adapter、plugin 分发时触发. 用 YAML adapter + 浏览器登录态把网站变成 CLI.
---

## 检查 / 安装

```bash
opencli --version
curl -fsSL https://github.com/forechoandlook/opencli_rs_plus/releases/latest/download/install.sh | bash
opencli update
opencli doctor
```

## 产品重心

**核心是 adapter。** 日常心智：

```text
跑:   opencli <site> <cmd>
找:   opencli adapter search
裁:   opencli adapter disable|enable
装:   opencli plugin install|update
身份: opencli kv …
常驻: opencli daemon start   # 插件/扩展时
```

已删除：tools / summary / feedback / job / FTS / find / history / adapter sync / hidden / socket exec。

## 模式

- **direct**: `opencli <site> <command>`（调试与日常抓取）
- **daemon**: `opencli daemon start` — plugin 热加载、扩展 action API
- **管理**: `adapter` / `plugin` / `kv`（enable/disable/kv 可不依赖 daemon）

```bash
opencli --help
opencli <site> --help
opencli adapter search zhihu
opencli adapter disable wikipedia
opencli adapter list --include-disabled
opencli plugin install forechoandlook/opencli-adapters
opencli daemon start
opencli daemon status
```

输出: `-f csv|table|json|yaml|md`，`--fields a,b,c`。

## KV

`~/.opencli-rs/kv.json`，只存身份字段。Key：`{site}:me.*`。

```bash
opencli kv get bilibili:me.mid
opencli kv list --prefix bilibili:
opencli kv clear --prefix xiaohongshu:
```

## 写 adapter

1. API 优先；Playwright/CDP 验证  
2. `adapters/<site>/<command>.yaml`  
3. 身份用 KV  
4. `cargo run -- <site> <cmd>`  
5. changelog；分发 opencli-adapters  

`retry: false` 风控；写操作 `--confirm`。

## 排查

| 现象 | 动作 |
|---|---|
| 找不到 adapter | `adapter search` / plugin / disable 列表 |
| mid 错 | `kv list` / clear prefix |
| 需登录 | 浏览器登录 + 扩展 |
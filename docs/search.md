# Adapter 发现与裁剪

| 方式 | 说明 |
|---|---|
| `opencli adapter search <query>` | 子串匹配 name/site/description/domain，打印可复制用法 |
| `opencli adapter list` | 浏览（`--include-disabled` 含已禁用） |
| `opencli adapter disable/enable` | 关掉不需要的命令或整站 |
| `opencli <site> --help` | 已知站点列命令（已 disable 的不会出现） |

## enable / disable

```bash
opencli adapter disable "zhihu hot"
opencli adapter disable zhihu/hot
opencli adapter disable wikipedia      # 整站
opencli adapter enable "zhihu hot"
opencli adapter list --include-disabled
cat ~/.opencli-rs/adapter_settings.json
```

禁用后：不进 help / search，direct 与 daemon 侧都不可执行。可不依赖 daemon（直接写 settings 文件）。

## 数据文件

| 文件 | 说明 |
|---|---|
| `~/.opencli-rs/adapter_settings.json` | disabled 列表 |
| `~/.opencli-rs/plugins/` | 已装插件 |
| `~/.opencli-rs/plugins.lock.json` | 插件来源 |
| `~/.opencli-rs/kv.json` | 身份 KV |

## Socket（daemon 内部）

| 方法 | 说明 |
|---|---|
| `adapter.search` / `list` | 发现 |
| `adapter.enable` / `disable` | 开关 |
| `adapter.reload` | 重载（plugin 变更后） |
| `plugin.*` | 插件管理 |
| `daemon.status` / `stop` / `ping` | 进程控制 |

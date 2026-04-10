
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

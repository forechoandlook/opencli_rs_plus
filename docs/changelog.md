# 2026-08-08

## 小红书与 B 站收藏全量导出

- **小红书 `favorites --all true`**：从个人收藏页以正常页面滚动等待站点自身继续加载，直到稳定无新增后导出全部已自然加载笔记；每次滚动会轮询页面状态最多 5 秒，避免下一页异步到达后误报只有首屏 20/30 条。不调用私有收藏接口、不伪造 `xsec` 签名。全量模式不受默认 20 条限制，不会静默截断在 200 条之前。结果新增 `favorite_time`（仅站点实际提供收藏时间时填写）和可复制的单条 `download_command`，保留每项的签名详情链接。
- **B 站 `favorite --all true`**：未传 `--folder-id` 时遍历当前账号全部收藏夹及每个收藏夹的所有分页；传入 `--folder-id` 时只全量导出该夹。全量模式不受默认 20 条限制，不会静默截断在 200 条之前。新增收藏夹、BV 号、收藏/发布时间、时长、简介、封面和单条下载命令。下载仍需用户显式执行 `bilibili download`，避免导出操作意外落盘海量视频。
- **已有页面复用与收藏入口**：只读收藏 adapter 的导航会优先借用用户已经打开且与目标 URL 完全一致的网页标签，不会刷新、纳入缓存或关闭它；`bilibili favorites` 现在直接输出具体视频，目录列表改为 `bilibili folders`；新增 `zhihu favorites`，遍历当前账号的收藏夹并输出具体条目。
- **B 站当前页操作**：`bilibili favorites` 匹配 `space.bilibili.com/<mid>/favlist`，其中用户 ID 为动态路径段；扩展可保存当前页已加载的收藏内容为 JSON。
- **小红书当前页操作**：收藏页路由更新为 `/user/profile/<userId>` 通配匹配，真实的 `tab=fav&subTab=note` 仍由提取器校验，避免遗漏不同账号的个人收藏页。
- **发布清理**：移除 CI 与 release workflow 中对已不存在的 `opencli-rs-ai` crate 的过期排除项；`opencli-rs-browser` 保留，负责浏览器 daemon、CDP 与扩展桥接。

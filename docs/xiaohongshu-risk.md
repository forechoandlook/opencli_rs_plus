# 小红书风控备忘（2026-08-16）

账号曾提示「被风控 / 发现自动化操作」。结论如下，后续跑小红书 adapter 先看本页。

## 结论

**最高风险是短时间内、登录态下对关注列表里多人连续打开主页并批量打 `user_posted`，不是单次 `following` 或点点 `ask`。**

触发参考：`following-archive` 批处理（约 18:30–18:35）对 **31** 个关注账号每人拉近 **100** 条笔记元数据（合计约 **1723** 条），人与人间隔约 **0.4s**，单用户约 8–12s。

## 高风险操作（按嫌疑）

1. **批量 `xiaohongshu user`**  
   `navigate` → `/user/profile/{id}` → 页内循环 `fetch('/api/sns/web/v1/user_posted')` → 立刻下一个人。服务端看到的是：同一 Cookie 会话、短时间大量不同 `user_id` 的列表翻页、几乎无笔记详情/停留。

2. **同一标签高速换主页**（`reuseExistingTab`）  
   连续 `/user/profile/A → B → C…`，缺人味混杂行为。

3. **`following` 走 Pinia `getUserFollow()` 后立刻全员刷笔记**  
   关注列表本身中等风险；与批量 `user_posted` 串成「先拉种子再扫库」更显眼。

4. **同日叠加 favorites / download / ask / creator**  
   次要加分项，不是主因。

## 相对低风险

- 单次 `xiaohongshu following`
- 偶发对**单个**用户 `user --limit 30`
- 偶发点点 `ask`

## 操作约束（降低暴露，不是绕过检测）

1. 不要一次扫完全部关注的近 100 条；改少量人 / `--incremental` 只拉新帖。
2. 人与人间隔用分钟级，单日主页切换设硬上限。
3. following 与 user 笔记拆开、隔天跑。
4. 被提示后先停；同账号继续 batch / 高频 `user_posted` 容易加重。
5. 用好 KV 缓存，避免无意义重复全量翻页。

## 相关路径

- Adapter：`xiaohongshu/user.yaml`（`user_posted`）、`xiaohongshu/following.yaml`（pinia）
- 批处理：`opencli batch` / `scripts/fetch_following_posts.py`
- 当日归档：`following-archive/xiaohongshu/`

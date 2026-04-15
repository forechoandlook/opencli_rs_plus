# Qwen Adapter Lessons

## 1. 先分清 API 侧事实和 DOM 侧事实

- `https://chat.qwen.ai/api/v2/configs/` 可以稳定拿到功能配置。
- 这批 key 当前确认可见: `slides`, `search`, `deep_research`, `learn`, `t2v`, `t2i`, `web_dev`, `mcp`, `travel`, `artifacts`。
- `feature_feature` 表示功能依赖的模式，例如 `web_dev -> thinking`，`deep_research -> research_mode`。
- `feature_file` 表示功能支持的输入类型，例如 `web_dev -> file,image,video,audio`。
- 所以 adapter 输出里不能把 “DOM 当前没点开菜单” 误报成 “功能不存在”。

结论：
- `Present` 这类字段优先基于 API 判断。
- DOM 解析失败时，应返回 `api_available_dom_unresolved`，不要返回 `missing`。

## 2. Qwen 页面菜单需要先用 Playwright 校准

- 用 `playwright-cli` 检查 Qwen 菜单比直接猜 DOM 稳得多。
- 当前匿名态页面核验结果：
  - `Deep Research`: found=true, disabled=false
  - `Create Image`: found=true, disabled=false
  - `Create Video`: found=true, disabled=true
  - `Web Dev`: found=true, disabled=false
  - `Slides`: found=true, disabled=true
- `Deep Research` 切换后 placeholder 是 `Describe the themes you want to research.`。
- `Web Dev` 切换后 placeholder 是 `Describe the web page you want to generate.`。

结论：
- 开发新的 Qwen adapter 时，先用 Playwright 把入口文案、disabled 状态、placeholder 校准，再固化到 YAML。
- 对 `More` 子菜单不要想当然，先在 Playwright 里确认真实交互。

## 3. opencli 和 Playwright 对同一页面的结果可能不一致

- 这次出现了一个典型分歧：
  - `playwright-cli` 能找到 `Deep Research`
  - `opencli qwen deep-research ...` 返回 `api_available_dom_unresolved`
- 这说明问题不在 Qwen 是否有这个功能，而在 opencli 当前浏览器执行链对弹层菜单的稳定解析。

结论：
- 出现这类问题时，先证明 “站点功能存在且可点”，再追 opencli 的页面执行差异。
- 不要把 opencli 的 DOM 失败归因到站点本身。

## 4. 图片生成要抓真实结果图 URL，不要抓示例图

- Qwen 生图结果的真实图片 URL 形态是：
  - `https://cdn.qwenlm.ai/output/.../image_gen/...png?...`
- 之前抓错图的原因，是误抓了示例区图片，而不是真实生成结果。
- 修复后应只匹配：
  - `img.qwen-image`
  - `img.ant-image-img.qwen-image`
  - `img[src*="cdn.qwenlm.ai/output/"]`
  - 并进一步过滤 `src` 中包含 `/image_gen/`

结论：
- 结果抓取必须基于真实输出 URL 规则，不要只靠“结果区出现图片”这种弱条件。

## 5. 长任务失败时先查 daemon client 超时

- 之前 Qwen 生图过程中出现过：
  - `error sending request for url (http://127.0.0.1:19825/command)`
- 根因不是页面超时，而是 browser daemon server 和 client 的 HTTP 超时不一致：
  - server 允许长时间等待
  - client 之前只等 30 秒

结论：
- 对 Qwen 这类长任务，先确认 opencli 的 daemon client 超时设置，而不是先盲目调大页面轮询。

## 6. 当前最合理的实现策略

- `status` / `menu` / `resources` / `check` 优先 API 化。
- `image` 这类创作流，允许 DOM 驱动，但必须做强校验：
  - prompt 是否真的写入
  - 是否遇到登录墙
  - 是否拿到真实输出资源
- `deep-research` / `web-dev` / `learn` / `travel` / `artifacts` / `search` 这类功能，当前应先实现为：
  - API 侧确认功能存在
  - DOM 侧尽量切换和提交
  - 一旦 DOM 不稳，明确返回 `api_available_dom_unresolved`

## 7. 后续优先级

1. 专门修 opencli 下 Qwen 菜单弹层定位，尤其是 `More` 子菜单。
2. 把 `Deep Research` 和 `Web Dev` 先做到稳定返回 `login_required` 或 `submitted`。
3. 再处理 `Learn` / `Travel` / `Artifacts` / `Search` 的结果抓取。
4. `Slides` / `Video` 在匿名态当前是 disabled，后续应在登录态下再验证，不要在未登录态硬做假成功。

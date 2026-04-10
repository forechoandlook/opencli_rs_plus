# Adapter Maintenance

目标不是继续人肉盯单个 adapter，而是先把全库按维护风险分类，再基于分类做回归。

## 分类标准

`ui_automation`
- `strategy: ui`
- 主要依赖页面交互、CDP、DOM、桌面应用状态
- 维护重点：页面结构回归、元素定位、时序

`api_bg_fetch`
- 使用 `bg_fetch`
- 适合优先做 API dump 和返回结构回归
- 维护重点：返回结构漂移、payload 大小、cookie 注入

`api_page_fetch`
- 先 `navigate`，再在页面里 `fetch(...)`
- 这是最值得持续审计的一类，里面会混着“可迁移到 bg_fetch”的读接口和“必须保留页面上下文”的接口
- 维护重点：识别哪些可以后台化，哪些依赖 referer/csrf/页面态

`api_direct_fetch`
- 不依赖页面导航的直接 `fetch(...)`
- 维护重点：接口可用性、速率、响应结构

`api_write_or_mutation`
- 写操作或带明显副作用的接口
- 维护重点：安全边界、幂等性、最小化自动回归范围

`page_navigation_dom`
- 有 `navigate` 但没有明显 API fetch
- 维护重点：页面结构和内容抓取稳定性

`other_pipeline`
- 不属于以上主要形态

## 当前资产

- 分类脚本: [scripts/classify-adapters.sh](/Users/zzwy/tmp/opencli-rs/scripts/classify-adapters.sh)
- 首版分类清单: [docs/generated/adapter-classification.tsv](/Users/zzwy/tmp/opencli-rs/docs/generated/adapter-classification.tsv)
- P1 回归清单: [docs/generated/regression-p1.tsv](/Users/zzwy/tmp/opencli-rs/docs/generated/regression-p1.tsv)
- 本地 smoke 回归脚本: [scripts/regression-smoke.sh](/Users/zzwy/tmp/opencli-rs/scripts/regression-smoke.sh)

## 推荐回归顺序

1. `api_bg_fetch`
2. `api_page_fetch`
3. `api_direct_fetch`
4. `page_navigation_dom`
5. `ui_automation`
6. `api_write_or_mutation`

原因：
- 前三类最容易积累 dump 样本，也最容易做结构回归
- `page_navigation_dom` 次之
- `ui_automation` 和写操作适合更谨慎、更小范围地回归

## 触发建议

本地显式触发:

```bash
scripts/regression-smoke.sh
scripts/regression-smoke.sh docs/generated/regression-p1.tsv
```

回归结果会输出到 `docs/generated/regression-smoke-*.tsv` 和 `docs/generated/regression-smoke-*.md`

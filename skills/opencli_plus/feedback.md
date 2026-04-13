## 作用

`opencli feedback` 用于把用户遇到的问题、错误结果、文档不准确之处记录到本地，并可选打开预填好的 GitHub issue 页面。

## 基本用法

```bash
opencli feedback "zhihu hot returns 403"
opencli feedback "contact filter mismatch" --body "friends filter returns public accounts"
opencli feedback "adapter summary inaccurate" --adapter "zhihu hot" --kind bad_description
opencli feedback "search endpoint changed" --kind broken --open
```

## 参数语义

- `title` 必填，短句即可，最好直接写用户可感知的症状。
- `--body` 或 `-m` 用来补充复现步骤、期望结果、实际结果、环境信息。
- `--adapter` 用来标记关联 adapter 名称，例如 `zhihu hot`。
- `--kind` 默认 `other`，可选 `broken`、`bad_description`、`other`。
- `--open` 会在浏览器中打开预填好的 GitHub issue 页面。

## 记录格式

默认写入 `~/.opencli-rs/feedback.jsonl`，每行一条 JSON 记录，字段包括：

- `id`
- `created_at`
- `version`
- `adapter`
- `kind`
- `title`
- `body`
- `opened_issue_url`

标题和 issue 页面会自动加上 `[feedback]` 前缀。若指定 adapter，issue title 里会包含 adapter 名称。

## 上报建议

- 如果是命令返回错误、结果为空、接口变更，优先标成 `broken`。
- 如果是 help 文案、summary、描述不准，优先标成 `bad_description`。
- 如果暂时不确定归类，保留 `other`，并在 body 里写清楚现象。
- `--open` 适合需要直接提交到 GitHub 的情况；只是本地留档则不需要。

## 排查和导出

- 先看 `~/.opencli-rs/feedback.jsonl` 是否已有重复记录。
- 如需批量处理，可直接按 JSONL 解析。

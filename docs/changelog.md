# 2026-04-15

- Added Qwen feature adapters for `check`, `deep-research`, `web-dev`, `learn`, `travel`, `artifacts`, `search`, `slides`, and `video`, with session-aware `missing/disabled/login_required` status reporting.
- Added a Qwen image-generation adapter for `opencli qwen image`, with login-aware fallback when the session cannot generate anonymously.
- Added downloadable Qwen resource adapters that index public image/video sample assets from `query-suggestion` config and save them via `download`.
- Added Qwen menu adapters to enumerate public feature/entry capabilities for `chat.qwen.ai` and `coder.qwen.ai`.
- Added Qwen public API status adapters for `chat.qwen.ai` and `coder.qwen.ai`, using browser-side fetch to read config/auth state without relying on logged-in DOM structure.
- Added adapter parse tests for both Qwen status adapters.

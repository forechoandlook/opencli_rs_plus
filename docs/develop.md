## 开发工作流程（CDP → YAML）

### Step 1: CDP探索阶段
使用playwright进行探索

### Step 2: YAML固化阶段
- 将验证过的JS逻辑写入 `evaluate` step
- 用 `cargo run -- site command` 端到端测试
- 对比CDP验证的结果，确保一致

### Step 3: 常见陷阱
1. **选错元素**：多个相似元素时，用特征组合过滤（如alt文本+blob URL+大尺寸）
2. **跨域/Blob限制**：Blob URL无法直接fetch，改用canvas从DOM元素读像素
3. **Navigation导致detach**：navigate到相同URL会失败，改在evaluate中检查位置
4. **轮询超时**：使用 `waitFor` 或轮询循环，给足充分的时间（网络不稳定）

## 添加新适配器
1. 在 `adapters/<site>/<command>.yaml` 创建YAML文件
2. 先用playwright探索页面、交互逻辑（见上述工作流）
3. 将验证过的代码写入pipeline中的evaluate步骤
4. 运行 `cargo build`
5. 测试：`cargo run -- site command [args]`

YAML schema 核心字段：

```yaml
site: <site_name>
name: <command_name>
strategy: public|cookie|header|intercept|ui
browser: true|false
timeoutSeconds: 180
args: {}  # 命令行参数
pipeline:
  - evaluate:
      "document.querySelectorAll('.item').length"   # 自动识别为 js 代码
      format: raw
      path: "./data/raw_{ts}.json"

    - evaluate:
        js: "async () => { return await fetch('/api').then(r=>r.json()) }"
        format: raw
        path: "./data/api_response_{ts}.json"

# 方式2: 独立 dump step（任意位置）
pipeline:
  - evaluate: "..."
  - dump: "./data/raw_{ts}.json"
  - map: { title: "${{ item.title }}" }
```

## 环境变量

- OPENCLI_VERBOSE, 启用日志输出（默认 info）；默认情况下日志保持静默，不会打印 adapter 加载细节
- OPENCLI_LOG_TIME, 日志时间格式：`minute`（默认，只到分钟）、`second`、`millisecond`/`ms`、`none`
- `--fields a,b,c`, 全局字段裁剪，只返回对象/对象数组里的顶层字段
- OPENCLI_DAEMON_PORT, 指定 daemon 端口 默认19825
- OPENCLI_CDP_ENDPOINT, 指定 CDP 直连端点（绕过 Daemon）
- OPENCLI_BROWSER_COMMAND_TIMEOUT, 命令超时（秒） 默认60
- OPENCLI_BROWSER_CONNECT_TIMEOUT, 浏览器连接超时（秒） 默认30
- OPENCLI_BROWSER_EXPLORE_TIMEOUT, Explore 超时（秒） 默认120
- OPENCLI_API_DUMP, 设为 `1`/`true` 后自动 dump `fetch` 和 `bg_fetch` 的原始 API 响应到磁盘，便于调试和批量识别可后台化的 adapter
- OPENCLI_API_DUMP_DIR, API dump 目录，默认 `./data/api-dumps`

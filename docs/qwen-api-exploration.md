# Qwen Chat API Exploration

## Summary

探索了 https://chat.qwen.ai/ 网站的API调用方式，目标是将基于DOM操作的adapter转换为基于API的实现。

## Key Findings

### Authentication
- **Token Storage**: localStorage 中的 `token` key
- **Token Type**: JWT format
- **Device ID**: localStorage 中的 `qwen_chat_device_id`
- **Note**: Token可能会过期，需要实现会话保留机制

### API Endpoints

#### 1. Chat Completions
- **Endpoint**: `/api/v2/chat/completions`
- **Method**: POST
- **Query Parameters**: `chat_id` (对话ID)
- **Headers**:
  - `Content-Type: application/json`
  - `Authorization: Bearer <token>` (可能的，需要验证)
  
**Request Body Example**:
```json
{
  "message": {
    "role": "user",
    "content": "提示词内容"
  },
  "functions": [],
  "function_call_history": [],
  "enable_internet_search": false,
  "mode": "general"
}
```

#### 2. Configuration API
- **Endpoint**: `/api/v2/configs/`
- **Method**: GET
- **Response**: Contains feature configurations and available modes

**Response Example**:
```json
{
  "data": {
    "function_entry": {},
    "features": {
      "feature_feature": {
        "search": [...],
        ...
      },
      "feature_file": {}
    }
  }
}
```

#### 3. Files API
- **Endpoint**: `/api/v2/files/`
- **Method**: GET
- **Response**: List of files/resources

### URL Structure
- **Chat Page**: `https://chat.qwen.ai/c/<chat_id>`
- **Chat ID Format**: UUID (e.g., `c60dca3a-415d-43d2-9d04-7740f89e57ae`)

### Session Cookies
关键的认证cookies:
- `_bl_uid`: Unique browser identifier
- `cnaui`: Alibaba user identifier
- `aui`: Another user identifier  
- `cna`: Alibaba session cookie

### LocalStorage Keys
- `token`: JWT认证令牌
- `qwen_chat_device_id`: 设备ID (UUID)
- `theme`: 主题设置
- `locale`: 语言设置
- `qwen-locale`: Qwen特定的语言设置
- `userRole`: 用户角色

## Session Preservation Strategy

根据用户要求"注意每一个qwen session 我都需要保留"，以下是推荐的实现方式：

### 选项 1: Daemon Mode with Persistent Browser
- 在daemon模式下运行浏览器实例
- 复用浏览器连接以保留会话
- 通过Socket API接收命令

### 选项 2: State Persistence
- 使用 `playwright-cli state-save` 保存认证状态
- 在后续请求中使用 `state-load` 恢复会话
- 存储localStorage和cookies

### 选项 3: Direct API with Token Refresh
- 通过API直接发送请求
- 实现token刷新机制
- 保存token到本地并定期验证

## Current Implementation (Browser-based)

现有adapters使用纯浏览器DOM操作：
- `qwen/search.yaml`: 通过DOM查找和点击UI元素
- `qwen/resources.yaml`: 通过评估JavaScript调用API
- 存在UI脆弱性：页面改动会导致adapter失效

## Recommended Next Steps

1. **API-based Implementation**: 
   - 改进token管理机制（刷新和持久化）
   - 实现完整的API请求封装
   - 支持会话复用

2. **Session Management**:
   - 实现session保存和加载
   - 或在daemon模式下维持长连接

3. **Error Handling**:
   - 处理token过期的情况
   - 处理网络错误和重试逻辑
   - 登录状态检测

## Testing Notes

- 浏览器会话创建: `playwright-cli open https://chat.qwen.ai/ --extension`
- 状态保存: `playwright-cli state-save qwen-state.json`
- 状态加载: `playwright-cli state-load qwen-state.json`
- 网络追踪: `playwright-cli tracing-start` / `tracing-stop`

## Important Observations

- Qwen Chat需要有效的登录会话
- API使用localhost相对路径 (e.g., `/api/v2/...`)
- 各个对话有独立的chat_id
- 系统支持多种功能模式（搜索、代码、思考等）

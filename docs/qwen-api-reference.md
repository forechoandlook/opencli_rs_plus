# Qwen Chat API Reference

## API 发现情况

通过 CDP (Chrome DevTools Protocol) 在浏览器上下文中调用 Qwen API，无需处理 token 机制。

### 认证模式
- **方式**: localStorage token
- **存储位置**: `localStorage.getItem('token')`
- **需求**: 某些 API 需要用户已登录

### 可用的 API 端点

#### 1. 配置 API
```
GET /api/v2/configs/
```
- **认证**: 不需要
- **返回**: 功能配置、特性开关、限制等
- **响应字段**:
  - `features.feature_feature` - 模式列表 (search, learn, deep_research等)
  - `features.feature_file` - 文件相关特性

#### 2. 文件 API
```
GET /api/v2/files/
```
- **认证**: 可能需要
- **返回**: 用户文件列表
- **响应字段**:
  - `data.list[].id` - 文件ID
  - `data.list[].name` - 文件名
  - `data.list[].content_type` - MIME类型
  - `data.list[].size` - 文件大小
  - `data.list[].status` - 文件状态

#### 3. 对话列表 API
```
GET /api/v2/chats/
```
- **认证**: ✅ **需要**（401 Unauthorized）
- **返回**: 用户的对话列表

#### 4. 对话创建 API (未验证)
```
POST /api/v2/chats/create/
Content-Type: application/json

{
  "title": "对话标题"
}
```
- **认证**: ✅ **需要**
- **返回**: 新创建的对话信息

#### 5. 聊天补全 API (已发现但未完全验证)
```
POST /api/v2/chat/completions?chat_id=<chat_id>
Content-Type: application/json

{
  "message": {
    "role": "user",
    "content": "提示词"
  },
  "functions": [],
  "function_call_history": [],
  "enable_internet_search": false,
  "mode": "general"
}
```
- **认证**: ✅ **需要**
- **参数**:
  - `chat_id` - 对话ID (URL参数)
  - `message` - 用户消息
  - `enable_internet_search` - 是否启用网络搜索
  - `mode` - 对话模式 (general/learning/research等)

### 不存在的 API (404 Not Found)
- `/api/v2/auth/status` - 不存在
- `/api/v2/user/info` - 不存在

## 认证机制

### 如何在 CLI 中使用这些 API

#### 方案 1: 使用 CDP + Daemon (推荐)
```yaml
# adapter 实现方式
site: qwen
name: search
browser: true
persistent: true

pipeline:
  - navigate: https://chat.qwen.ai/
  - wait: 2
  - evaluate: |
      (async () => {
        // 检查认证
        const token = localStorage.getItem('token');
        if (!token) {
          return { error: 'Not logged in' };
        }
        
        // 调用 API (浏览器会自动发送认证)
        const res = await fetch('/api/v2/chats/');
        return await res.json();
      })()
```

**优点**:
- 无需处理 token 管理
- 浏览器自动处理 cookies 和认证
- 支持会话复用 (daemon 模式)

**缺点**:
- 需要保持浏览器会话
- 慢于直接 API 调用

#### 方案 2: 直接 API 调用 (复杂)
需要解决的问题:
1. 如何获取有效的 token
2. Token 过期处理
3. CSRF token 或其他安全机制

## 实现路径

根据 CLAUDE.md 的要求"保留每一个 qwen session"：

1. **启用 daemon 模式** - 维持长连接的浏览器
2. **使用 CDP 方式** - 在浏览器上下文中执行 API 调用
3. **Session 复用** - 多个命令共享同一个浏览器会话

## 下一步

需要测试:
1. ✅ `/api/v2/configs/` - 可用
2. ❓ `/api/v2/chats/` - 需要认证
3. ❓ `/api/v2/chat/completions` - 需要认证
4. ❓ POST 请求是否需要特殊的请求头

## 浏览器环境中的 API 调用示例

```javascript
// 在浏览器的 JavaScript 上下文中执行
const res = await fetch('/api/v2/chats/');
const data = await res.json();
console.log(data);
```

所有认证都由浏览器自动处理，无需手动设置 Authorization 头。

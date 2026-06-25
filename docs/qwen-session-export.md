# Qwen 对话导出方案 (Session Export)

## 需求
- 导出 Qwen Chat 对话内容为 JSONL 格式
- 支持保存整个对话会话内容
- 可以从对话 URL 中提取数据

## API 发现

### 对话消息获取 API (需要验证)
```
GET /api/v2/chat/messages?chat_id=<chat_id>
GET /api/v2/chat/history?chat_id=<chat_id>
GET /api/v2/chats/<chat_id>/messages
```

**参数**:
- `chat_id` - 对话ID (从URL提取: `https://chat.qwen.ai/c/<chat_id>`)

## 实现方案

### 方案 A: 通过 DOM 提取 (最可靠)
```yaml
site: qwen
name: export
description: Export Qwen conversation to JSONL
domain: chat.qwen.ai
strategy: public
browser: true

args:
  chat_url:
    positional: true
    type: string
    required: true
    description: Chat URL (e.g., https://chat.qwen.ai/c/xxx)
  output:
    type: string
    default: ""
    description: Output file path (default ./qwen-export.jsonl)

columns: [status, messages_count, output_file]

pipeline:
  - navigate: ${{ args.chat_url }}
  - wait: 3
  - evaluate: |
      (async () => {
        // 从DOM提取对话内容
        const chatId = window.location.pathname.split('/c/')[1];
        
        // 验证登录状态
        const token = localStorage.getItem('token');
        if (!token) {
          return [{
            status: 'error',
            messages_count: 0,
            output_file: 'Not logged in'
          }];
        }
        
        // 获取所有消息元素
        const messages = [];
        const messageElements = document.querySelectorAll('[role="article"], main > div > div');
        
        messageElements.forEach((el, idx) => {
          const text = el.innerText || el.textContent;
          if (text && text.length > 0) {
            messages.push({
              index: idx,
              role: 'assistant', // 需要根据UI推断
              content: text.trim()
            });
          }
        });
        
        return [{
          status: 'success',
          messages_count: messages.length,
          output_file: ${{ args.output | json }},
          messages: messages
        }];
      })()
  - dump: ${{ args.output | default: './qwen-export.jsonl' }}
```

### 方案 B: 通过 API 获取 (需要真实API验证)
```javascript
// 伪代码，需要先验证API响应格式
const chatId = 'a3618277-182e-4864-9d95-5f77398bda4d';
const response = await fetch(`/api/v2/chat/messages?chat_id=${chatId}`);
const data = await response.json();

// 转换为 JSONL 格式
const jsonl = data.messages
  .map(msg => JSON.stringify(msg))
  .join('\n');
```

## JSONL 格式定义

```jsonl
{"role":"user","content":"你好","timestamp":"2026-04-15T10:00:00Z","index":0}
{"role":"assistant","content":"你好！有什么我可以帮你的吗？","timestamp":"2026-04-15T10:00:05Z","index":1}
{"role":"user","content":"介绍一下Qwen","timestamp":"2026-04-15T10:01:00Z","index":2}
{"role":"assistant","content":"Qwen是...","timestamp":"2026-04-15T10:01:10Z","index":3}
```

**字段**:
- `role` - "user" 或 "assistant"
- `content` - 消息内容
- `timestamp` - 发送时间 (ISO8601)
- `index` - 消息顺序
- `chat_id` - 对话ID (可选)
- `metadata` - 附加信息如搜索状态等 (可选)

## 使用示例

```bash
# 导出对话到默认文件 (./qwen-export.jsonl)
opencli qwen export https://chat.qwen.ai/c/a3618277-182e-4864-9d95-5f77398bda4d

# 导出到指定文件
opencli qwen export https://chat.qwen.ai/c/a3618277-182e-4864-9d95-5f77398bda4d -o ./my-chat.jsonl

# 输出示例
status                messages_count  output_file
success               15              ./qwen-export.jsonl
```

## 下一步

1. **验证 API** - 确认对话消息的API端点和响应格式
2. **DOM 解析** - 如果API不可用，实现基于DOM的消息提取
3. **格式处理** - 处理消息中的图片、代码块等特殊格式
4. **会话复用** - 使用daemon模式在多次导出中复用浏览器会话

## 其他可能的API端点

基于Qwen的对话功能，可能还有：
- `/api/v2/chats/<chat_id>/` - 获取对话元数据
- `/api/v2/chats/<chat_id>/messages/` - 获取消息列表
- `/api/v2/download/chat/<chat_id>` - 下载对话（可能存在）

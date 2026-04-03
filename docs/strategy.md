
- public 默认不使用浏览器，直接使用 fetch 获取数据
- cookie 会自动带上cookie, fetch() 带 credentials: 'include' 自动携带 cookie
- header 会自动提取csrf token并附加到Header
- intercept 需要站点特定实现（手动 hook/拦截）
- ui 需要站点特定实现（模拟 UI 交互）
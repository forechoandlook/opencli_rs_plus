async function zhihuGet(url) {
  const response = await fetch(url, { credentials: 'include', headers: { 'x-requested-with': 'fetch' } });
  const payload = await response.json().catch(() => ({}));
  if (!response.ok || payload?.error) {
    const msg = String(payload?.error?.message || response.status || '');
    if (/注销|不存在|找不到|没有权限|未找到|已重置|reseted/i.test(msg) || response.status === 404) {
      return { data: [], paging: { is_end: true }, _soft_empty: true, _message: msg };
    }
    if (/登录|AUTH/i.test(msg) || response.status === 401 || response.status === 403) {
      throw new Error('AUTH_REQUIRED: zhihu.com ' + msg);
    }
    if (response.status === 429 || /频繁|太快/.test(msg)) {
      throw new Error('RATE_LIMIT: ' + msg);
    }
    throw new Error('知乎接口失败: ' + msg + ' ' + url);
  }
  return payload;
}
function zhihuStrip(html) {
  return String(html || '').replace(/<[^>]+>/g, '').replace(/&nbsp;/g, ' ').replace(/&amp;/g, '&').replace(/\s+/g, ' ').trim();
}
function zhihuIso(value) {
  const n = Number(value || 0);
  return Number.isFinite(n) && n > 0 ? new Date(n * 1000).toISOString().slice(0, 10) : '';
}

async function waitPiniaUser(maxTries) {
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  const tries = Math.max(Number(maxTries) || 12, 1);
  for (let i = 0; i < tries; i++) {
    const pinia = document.querySelector('#app')?.__vue_app__?.config?.globalProperties?.$pinia;
    const user = pinia?._s?.get?.('user');
    if (user && typeof user.getUserFollow === 'function') return user;
    await sleep(400);
  }
  throw new Error('AUTH_REQUIRED: xiaohongshu.com 未找到用户状态，请确认已登录网页版');
}

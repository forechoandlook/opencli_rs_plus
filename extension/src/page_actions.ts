/**
 * Generic direct-current-tab adapter runtime.
 *
 * The daemon supplies only a YAML-declared activeTab plan. This runtime never
 * navigates, scrolls, clicks, opens a window, or calls the normal task engine.
 */

import * as cdp from './cdp';

type JsonObject = Record<string, unknown>;

export type ActiveTabPlan = {
  usePipeline?: boolean;
  extract?: string;
};

export type DirectContextAction = {
  adapter: string;
  activeTab: ActiveTabPlan;
  args?: Record<string, string>;
  pipeline?: unknown[];
};

export type DirectActionResult = { message: string; downloads: number };

export async function runCurrentPageAction(action: DirectContextAction, expectedUrl: string): Promise<DirectActionResult> {
  const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
  if (!tab?.id || !tab.url) throw new Error('未找到当前网页标签');
  if (tab.url !== expectedUrl) throw new Error('页面已变化；请关闭弹窗后重新打开再执行');

  try {
    const data = action.activeTab.extract
      ? await cdp.evaluateAsync(tab.id, action.activeTab.extract)
      : await runPipeline(tab.id, action.pipeline ?? [], action.args ?? {});
    return await saveActionResult(action.adapter, data);
  } finally {
    await cdp.detach(tab.id);
  }
}

async function runPipeline(tabId: number, pipeline: unknown[], args: Record<string, string>): Promise<unknown> {
  if (!pipeline.length) throw new Error('该当前页面操作没有可执行的 YAML pipeline');
  let data: unknown = null;
  let downloaded = false;

  for (const rawStep of pipeline) {
    if (!isObject(rawStep)) throw new Error('当前页面 pipeline 包含无效步骤');
    if ('navigate' in rawStep) {
      // Explicitly skipped: this mode operates only on the page the user opened.
      continue;
    }
    const evaluate = evaluateCode(rawStep);
    if (evaluate) {
      data = await cdp.evaluateAsync(tabId, renderTemplate(evaluate, args, data));
      continue;
    }
    if ('limit' in rawStep) {
      if (Array.isArray(data)) data = data.slice(0, Number(renderValue(rawStep.limit, args, data)) || 0);
      continue;
    }
    if ('map' in rawStep) {
      const mapSpec = rawStep.map;
      if (!Array.isArray(data) || !isObject(mapSpec)) throw new Error('当前页面 map 需要数组数据和对象配置');
      data = data.map((item, index) => Object.fromEntries(
        Object.entries(mapSpec).map(([key, value]) => [key, renderValue(value, args, data, item, index)]),
      ));
      continue;
    }
    if ('download' in rawStep) {
      if (!isObject(rawStep.download)) throw new Error('当前页面 download 配置无效');
      await downloadMediaBatch(data, rawStep.download);
      downloaded = true;
      continue;
    }
    // Existing pipelines can contain navigation-only transport steps. They are
    // intentionally rejected rather than silently issuing background requests.
    const unsupported = Object.keys(rawStep).find((key) => !['map', 'dump'].includes(key));
    if (unsupported) throw new Error(`当前页面模式不支持 YAML 步骤：${unsupported}`);
  }

  return { data, downloaded };
}

async function saveActionResult(adapter: string, result: unknown): Promise<DirectActionResult> {
  const wrapped = isObject(result) && 'data' in result && 'downloaded' in result ? result : { data: result, downloaded: false };
  if (wrapped.downloaded === true) {
    const count = mediaCount(wrapped.data) + 1;
    return { message: `已从当前页面开始下载 ${count} 个文件`, downloads: count };
  }

  const name = `${safeName(adapter)}/${new Date().toISOString().replace(/[:.]/g, '-')}.json`;
  await downloadText(`OpenCLI/${name}`, JSON.stringify(wrapped.data, null, 2), 'application/json');
  const count = Array.isArray(wrapped.data) ? wrapped.data.length : 1;
  return { message: `已保存当前页面提取结果（${count} 条）`, downloads: 1 };
}

async function downloadMediaBatch(data: unknown, _config: JsonObject): Promise<void> {
  if (_config.type !== undefined && _config.type !== 'media-batch') {
    throw new Error(`当前页面模式仅支持 media-batch 下载，收到：${String(_config.type)}`);
  }
  if (!isObject(data)) throw new Error('media-batch 需要 evaluate 返回对象');
  const noteId = safeName(String(data.noteId ?? data.id ?? 'item'));
  const folder = `OpenCLI/${noteId}`;
  await downloadText(`${folder}/note.md`, renderMarkdown(data), 'text/markdown');

  const items = Array.isArray(data.items) ? data.items : [];
  for (const [index, item] of items.entries()) {
    if (!isObject(item) || typeof item.url !== 'string' || !/^https?:\/\//i.test(item.url)) continue;
    const type = item.type === 'video' ? 'video' : 'image';
    await chrome.downloads.download({
      url: item.url,
      filename: `${folder}/${String(index + 1).padStart(3, '0')}-${type}${extensionFor(item.url, type)}`,
      conflictAction: 'uniquify',
      saveAs: false,
    });
  }
}

function renderMarkdown(data: JsonObject): string {
  const comments = Array.isArray(data.comments) ? data.comments.filter(isObject) : [];
  const items = Array.isArray(data.items) ? data.items.filter(isObject) : [];
  const lines = [
    `# ${String(data.title ?? 'untitled')}`,
    '',
    `- 作者：${String(data.author ?? 'unknown')}`,
    data.noteType ? `- 类型：${String(data.noteType)}` : '',
    data.sourceUrl ? `- 来源：${redactUrl(String(data.sourceUrl))}` : '',
    '',
    '## 正文',
    '',
    String(data.content ?? data.desc ?? ''),
    '',
    '## 已加载评论',
    '',
    ...comments.map((comment) => `- ${String(comment.author ?? '匿名')}：${String(comment.content ?? '')}`),
    '',
    '## 媒体',
    '',
    ...items.map((item, index) => `- ${String(index + 1).padStart(3, '0')}-${String(item.type ?? 'image')}${extensionFor(String(item.url ?? ''), item.type === 'video' ? 'video' : 'image')}`),
    '',
  ];
  return lines.filter((line, index) => line || index > 0).join('\n');
}

function evaluateCode(step: JsonObject): string | null {
  if (typeof step.evaluate === 'string') return step.evaluate;
  if (isObject(step.evaluate) && typeof step.evaluate.js === 'string') return step.evaluate.js;
  return null;
}

function renderTemplate(source: string, args: Record<string, string>, data: unknown, item?: unknown, index?: number): string {
  return source.replace(/\$\{\{\s*([^}]+?)\s*\}\}/g, (_match, expression: string) => {
    const value = resolveExpression(expression, args, data, item, index);
    return expression.includes('| json') ? JSON.stringify(value) : String(value ?? '');
  });
}

function renderValue(value: unknown, args: Record<string, string>, data: unknown, item?: unknown, index?: number): unknown {
  return typeof value === 'string' ? renderTemplate(value, args, data, item, index) : value;
}

function resolveExpression(expression: string, args: Record<string, string>, data: unknown, item?: unknown, index?: number): unknown {
  const target = expression.split('|')[0].trim();
  const defaultMatch = expression.match(/\|\s*default\((['"]?)(.*?)\1\)/);
  let value: unknown;
  const argMatch = target.match(/^args\[['"]([^'"]+)['"]\]$/) || target.match(/^args\.([\w-]+)$/);
  if (argMatch) value = args[argMatch[1]];
  else if (target === 'data') value = data;
  else if (target.startsWith('data.')) value = getPath(data, target.slice(5));
  else if (target === 'item') value = item;
  else if (target.startsWith('item.')) value = getPath(item, target.slice(5));
  else if (target === 'index') value = index;
  else if (/^index\s*\+\s*\d+$/.test(target)) value = (index ?? 0) + Number(target.split('+')[1].trim());
  if ((value === undefined || value === null || value === '') && defaultMatch) value = defaultMatch[2];
  return value;
}

function getPath(value: unknown, path: string): unknown {
  return path.split('.').reduce<unknown>((current, key) => isObject(current) ? current[key] : undefined, value);
}

function mediaCount(data: unknown): number {
  return isObject(data) && Array.isArray(data.items) ? data.items.length : 0;
}

async function downloadText(filename: string, content: string, mime: string): Promise<void> {
  const url = `data:${mime};charset=utf-8,${encodeURIComponent(content)}`;
  await chrome.downloads.download({ url, filename, conflictAction: 'uniquify', saveAs: false });
}

function extensionFor(url: string, type: 'image' | 'video'): string {
  try {
    const match = new URL(url).pathname.toLowerCase().match(/\.(jpg|jpeg|png|webp|gif|avif|mp4|webm)$/);
    return `.${match?.[1] === 'jpeg' ? 'jpg' : match?.[1] ?? (type === 'video' ? 'mp4' : 'jpg')}`;
  } catch {
    return type === 'video' ? '.mp4' : '.jpg';
  }
}

function redactUrl(raw: string): string {
  try {
    const url = new URL(raw);
    url.searchParams.delete('xsec_token');
    return url.toString();
  } catch {
    return raw;
  }
}

function safeName(value: string): string {
  return value.replace(/[^a-zA-Z0-9._-]/g, '_').slice(0, 100) || 'item';
}

function isObject(value: unknown): value is JsonObject {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

export const __test__ = { renderTemplate, renderValue, renderMarkdown };

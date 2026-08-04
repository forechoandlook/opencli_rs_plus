import { describe, expect, it } from 'vitest';
import { __test__ } from './page_actions';

describe('current-page YAML template runtime', () => {
  it('renders current-url arguments as JSON-safe JavaScript values', () => {
    const rendered = __test__.renderTemplate(
      "const url = ${{ args['note-id'] | json }};",
      { 'note-id': 'https://example.com/explore/a?x=1' },
      null,
    );
    expect(rendered).toBe('const url = "https://example.com/explore/a?x=1";');
  });

  it('renders adapter data, defaults, items, and indexes for reusable YAML steps', () => {
    expect(__test__.renderTemplate('${{ data.noteId | json }}', {}, { noteId: 'note-1' })).toBe('"note-1"');
    expect(__test__.renderTemplate('${{ args.limit | default(10) }}', {}, null)).toBe('10');
    expect(__test__.renderTemplate('${{ item.title }}-${{ index + 1 }}', {}, null, { title: 'hello' }, 0)).toBe('hello-1');
  });

  it('keeps adapter-provided body and loaded comments in downloaded Markdown', () => {
    const markdown = __test__.renderMarkdown({
      title: '标题',
      author: '作者',
      content: '正文内容',
      comments: [{ author: '评论者', content: '评论内容' }],
      items: [],
    });
    expect(markdown).toContain('正文内容');
    expect(markdown).toContain('评论者：评论内容');
  });
});

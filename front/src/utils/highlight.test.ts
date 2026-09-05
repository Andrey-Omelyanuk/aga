import { describe, expect, it } from 'vitest';
import { highlightText } from '@/utils/highlight';

describe('highlightText', () => {
  it('подсвечивает код известного языка токенами', () => {
    const html = highlightText('fn main() {}', 'rust');
    expect(html).toContain('<span class="hljs-');
  });

  it('экранирует HTML-символы внутри подсвеченного кода', () => {
    const html = highlightText('const s = "<b>";', 'javascript');
    expect(html).not.toContain('<b>');
    expect(html).toContain('&lt;');
  });

  it('возвращает обычный экранированный текст для plaintext и неизвестного языка', () => {
    expect(highlightText('<div>x</div>', 'plaintext')).toBe('&lt;div&gt;x&lt;/div&gt;');
    expect(highlightText('<div>x</div>', 'unknownlang')).toBe('&lt;div&gt;x&lt;/div&gt;');
  });
});
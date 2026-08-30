import { describe, expect, it } from 'vitest';
import { escapeHtml } from '@/lib/format';

describe('escapeHtml', () => {
  it('escapes HTML special characters', () => {
    expect(escapeHtml('<script>alert("x&y")</script>')).toBe(
      '&lt;script&gt;alert(&quot;x&amp;y&quot;)&lt;/script&gt;',
    );
  });

  it('converts null/undefined-ish to string', () => {
    expect(escapeHtml(null)).toBe('null');
    expect(escapeHtml(undefined)).toBe('undefined');
  });
});
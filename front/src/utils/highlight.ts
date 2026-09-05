// Берем common-набор highlight.js (а не полный ~1 МБ): им покрыты все языки
// LANG_MAP кроме dockerfile и protobuf — их регистрируем отдельно.
import hljs from 'highlight.js/lib/common';
import dockerfile from 'highlight.js/lib/languages/dockerfile';
import protobuf from 'highlight.js/lib/languages/protobuf';
import { escapeHtml } from '@/utils/html';

hljs.registerLanguage('dockerfile', dockerfile);
hljs.registerLanguage('protobuf', protobuf);

export function highlightText(code: string, lang: string): string {
  if (lang === 'plaintext' || !hljs.getLanguage(lang)) {
    return escapeHtml(code);
  }
  return hljs.highlight(code, { language: lang }).value;
}
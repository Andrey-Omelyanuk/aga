import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github-dark.css';

/** Рендер markdown с GFM (таблицы, зачёркивание, списки задач) и подсветкой
 * кода (highlight.js через rehype-highlight). Тёмные код-блоки — чтобы
 * код отличался от текста; подсветка — светлая тема github. */
export const Markdown = ({ content }: { content: string }) => (
  <div className="prose prose-sm max-w-none prose-pre:rounded-md prose-pre:bg-slate-900 prose-pre:text-slate-100">
    <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
      {content}
    </ReactMarkdown>
  </div>
);
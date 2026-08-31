import { observer } from 'mobx-react-lite';
import { fileBrowser } from '@/models/files';
import { EmptyState } from '@/components/ui/tabs';
import { escapeHtml } from '@/utils/html';

export const FileViewer = observer(() => {
  if (!fileBrowser.currentPath) {
    return (
      <div className="flex-1">
        <EmptyState>Откройте папку или файл в дереве</EmptyState>
      </div>
    );
  }
  const content = fileBrowser.content;
  return (
    <div className="flex flex-1 flex-col bg-white">
      <div className="border-b border-slate-200 px-5 py-3 font-mono text-sm text-slate-500">
        📄 {fileBrowser.currentPath}
      </div>
      <div className="flex-1 overflow-auto p-5">
        {fileBrowser.loading || !content ? (
          <EmptyState>Загрузка…</EmptyState>
        ) : content.objectUrl ? (
          content.contentType.startsWith('image/') ? (
            <img src={content.objectUrl} alt={fileBrowser.currentPath} className="max-w-full rounded-lg" />
          ) : content.contentType.startsWith('video/') ? (
            <video controls src={content.objectUrl} className="max-w-full rounded-lg" />
          ) : (
            <audio controls src={content.objectUrl} />
          )
        ) : content.text !== undefined ? (
          <pre className="m-0 overflow-auto rounded-lg border border-slate-200 bg-slate-50 p-3.5">
            <code dangerouslySetInnerHTML={{ __html: escapeHtml(content.text) }} />
          </pre>
        ) : (
          <EmptyState>Не удалось прочитать файл</EmptyState>
        )}
      </div>
    </div>
  );
});
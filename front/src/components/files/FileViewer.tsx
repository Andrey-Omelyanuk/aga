import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
import { fileBrowser, langFor } from '@/models/files';
import { EmptyState } from '@/components/ui/tabs';
import { highlightText } from '@/utils/highlight';

export const FileViewer = observer(() => {
  const { t } = useTranslation();
  if (!fileBrowser.currentPath) {
    return (
      <div className="flex-1">
        <EmptyState>{t('files.openFileHint')}</EmptyState>
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
          <EmptyState>{t('files.loading')}</EmptyState>
        ) : content.objectUrl ? (
          content.contentType.startsWith('image/') ? (
            <img src={content.objectUrl} alt={fileBrowser.currentPath} className="max-w-full rounded-lg" />
          ) : content.contentType.startsWith('video/') ? (
            <video controls src={content.objectUrl} className="max-w-full rounded-lg" />
          ) : (
            <audio controls src={content.objectUrl} />
          )
        ) : content.text !== undefined ? (
          <pre className="m-0 overflow-auto rounded-lg border border-slate-200 bg-slate-900 p-3.5">
            <code
              className="hljs"
              dangerouslySetInnerHTML={{
                __html: highlightText(content.text, langFor(fileBrowser.currentPath)),
              }}
            />
          </pre>
        ) : (
          <EmptyState>{t('files.failedToRead')}</EmptyState>
        )}
      </div>
    </div>
  );
});
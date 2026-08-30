import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Select } from '@/components/ui/select';
import { EmptyState } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';
import { fileBrowser, type TreeEntry } from '@/store';
import { http } from '@/models/registry';
import { escapeHtml } from '@/lib/format';

export const Files = observer(function Files() {
  const store = useStore();
  const fb = fileBrowser;

  return (
    <div className="flex h-full overflow-hidden">
      <div className="flex w-80 flex-col border-r border-slate-200 bg-white">
        <div className="p-4 pb-1">
          <Select
            value={fb.workstationId ? String(fb.workstationId) : ''}
            onChange={(e) =>
              fb.selectWorkstation(e.target.value ? Number(e.target.value) : null)
            }
          >
            <option value="">Выберите воркстейшн...</option>
            {store.workstations
              .filter((ws) => ws.isReady)
              .map((ws) => (
                <option key={ws.id} value={String(ws.id)}>
                  {ws.name} ({ws.state})
                </option>
              ))}
          </Select>
        </div>
        <div className="flex-1 overflow-auto px-4 py-3 text-sm">
          {fb.workstationId === null ? (
            <EmptyState>Выберите воркстейшн</EmptyState>
          ) : fb.entries.length === 0 ? (
            <EmptyState>Папка пуста</EmptyState>
          ) : (
            fb.entries.map((entry) => (
              <FileTreeEntry key={entry.path} entry={entry} onOpen={fb.openFile} />
            ))
          )}
        </div>
      </div>
      <FileViewer />
    </div>
  );
});

function FileTreeEntry({
  entry,
  onOpen,
}: {
  entry: TreeEntry;
  onOpen: (entry: TreeEntry) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<TreeEntry[]>([]);

  const toggle = async () => {
    if (!expanded) {
      if (children.length === 0 && fileBrowser.workstationId !== null) {
        const tree = await http.json<{ entries: TreeEntry[] }>(
          `/workstations/${fileBrowser.workstationId}/tree?path=${encodeURIComponent(entry.path)}`,
        );
        setChildren(tree.entries);
      }
    }
    setExpanded(!expanded);
  };

  if (entry.kind === 'dir') {
    return (
      <div>
        <button
          className="ml-0 w-full cursor-pointer rounded px-1.5 py-0.5 text-left font-semibold text-slate-800 hover:bg-slate-100"
          onClick={toggle}
        >
          {expanded ? '▼' : '▶'} 📁 {entry.name}
        </button>
        {expanded && (
          <div className="pl-4">
            {children.length === 0 && (
              <div className="px-1.5 py-0.5 text-slate-400">Папка пуста</div>
            )}
            {children.map((child) => (
              <FileTreeEntry key={child.path} entry={child} onOpen={onOpen} />
            ))}
          </div>
        )}
      </div>
    );
  }
  return (
    <button
      className="w-full cursor-pointer rounded px-1.5 py-0.5 text-left hover:bg-slate-100"
      onClick={() => onOpen(entry)}
    >
      📄 {entry.name}
    </button>
  );
}

const FileViewer = observer(function FileViewer() {
  const fb = fileBrowser;
  if (!fb.currentPath) {
    return (
      <div className="flex-1">
        <EmptyState>Откройте папку или файл в дереве</EmptyState>
      </div>
    );
  }
  const content = fb.content;
  return (
    <div className="flex flex-1 flex-col bg-white">
      <div className="border-b border-slate-200 px-5 py-3 font-mono text-sm text-slate-500">
        📄 {fb.currentPath}
      </div>
      <div className="flex-1 overflow-auto p-5">
        {fb.loading || !content ? (
          <EmptyState>Загрузка…</EmptyState>
        ) : content.objectUrl ? (
          content.contentType.startsWith('image/') ? (
            <img src={content.objectUrl} alt={fb.currentPath} className="max-w-full rounded-lg" />
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
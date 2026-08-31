import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { fileBrowser, TreeEntry } from '@/models/files';
import http from '@/services/http';
import { EmptyState } from '@/components/ui/tabs';

function TreeEntryView({ entry, onOpen }: { entry: TreeEntry; onOpen: (e: TreeEntry) => void }) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<TreeEntry[]>([]);

  const toggle = async () => {
    if (!expanded && children.length === 0 && fileBrowser.workstationId !== null) {
      const response = await http.get<{ entries: TreeEntry[] }>(
        `/workstations/${fileBrowser.workstationId}/tree?path=${encodeURIComponent(entry.path)}`,
      );
      setChildren(response.data.entries);
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
              <TreeEntryView key={child.path} entry={child} onOpen={onOpen} />
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

export const FileTree = observer(() => {
  if (fileBrowser.workstationId === null) {
    return <EmptyState>Выберите воркстейшн</EmptyState>;
  }
  if (fileBrowser.entries.length === 0) {
    return <EmptyState>Папка пуста</EmptyState>;
  }
  return (
    <div>
      {fileBrowser.entries.map((entry) => (
        <TreeEntryView key={entry.path} entry={entry} onOpen={(e) => fileBrowser.openFile(e)} />
      ))}
    </div>
  );
});
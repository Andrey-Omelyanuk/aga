import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Card, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { EmptyState } from '@/components/ui/tabs';
import http from '@/services/http';
import { cn } from '@/lib/utils';

export interface ChangesSummary {
  base: string | null;
  changed: boolean;
  diff: string;
}

/** Строчный рендер unified-диффа git: мета (заголовки файлов) серым,
 * `@@`-хунки заголовками, добавленное зелёным, удалённое красным. */
function DiffLines({ diff }: { diff: string }) {
  const lines = diff.split('\n');
  return (
    <pre className="overflow-x-auto rounded-md border border-slate-200 bg-slate-50 text-xs leading-relaxed">
      {lines.map((line, i) => {
        let cls = 'text-slate-700';
        if (
          line.startsWith('diff --git ') ||
          line.startsWith('index ') ||
          line.startsWith('--- ') ||
          line.startsWith('+++ ') ||
          line.startsWith('new file mode') ||
          line.startsWith('deleted file mode') ||
          line.startsWith('Binary files')
        ) {
          cls = 'text-slate-500';
        } else if (line.startsWith('@@')) {
          cls = 'bg-slate-100 font-semibold text-slate-500';
        } else if (line.startsWith('+')) {
          cls = 'bg-green-50 text-green-800';
        } else if (line.startsWith('-')) {
          cls = 'bg-red-50 text-red-700';
        }
        return (
          <div key={i} className={cn('whitespace-pre px-3', cls)}>
            {line || ' '}
          </div>
        );
      })}
    </pre>
  );
}

const ChangesPage = observer(() => {
  const { id } = useParams<{ id: string }>();
  const wsId = id !== undefined && /^\d+$/.test(id) ? Number(id) : null;
  const [summary, setSummary] = useState<ChangesSummary | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (wsId === null) return;
    setSummary(null);
    setError(false);
    http
      .get<ChangesSummary>(`/workstations/${wsId}/changes`)
      .then((res) => setSummary(res.data))
      .catch(() => setError(true));
  }, [wsId]);

  return (
    <div>
      <Link to="/chat" className="text-xs text-blue-600 hover:underline">
        ← Сессии
      </Link>
      <Card>
        <CardTitle>
          Изменения <Badge variant="info">#{wsId ?? '—'}</Badge>
        </CardTitle>
        {error ? (
          <EmptyState>Не удалось получить изменения</EmptyState>
        ) : summary === null ? (
          <EmptyState>Загрузка…</EmptyState>
        ) : !summary.changed ? (
          <EmptyState>Изменений нет</EmptyState>
        ) : (
          <div>
            {summary.base && (
              <p className="mb-2 text-xs text-slate-400">Сравнение с {summary.base}</p>
            )}
            <DiffLines diff={summary.diff} />
          </div>
        )}
      </Card>
    </div>
  );
});

export default ChangesPage;
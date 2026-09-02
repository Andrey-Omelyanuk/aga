import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { Link, useLocation, useParams } from 'react-router-dom';
import { Card, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { EmptyState } from '@/components/ui/tabs';
import { Diff } from '@/components/core/Diff';
import http from '@/services/http';
import { formatTime } from '@/utils/dates';
import type { CapabilityHistoryEntry } from '@/models/project';

type RouteKind = 'skills' | 'commands';

const ACTION_LABEL: Record<CapabilityHistoryEntry['action'], string> = {
  create: 'создал',
  update: 'изменил содержимое',
  rename: 'переименовал',
  delete: 'удалил',
};

/** Дифф содержимого этой записи к предыдущей: предыдущее содержимое берём из
 * соседней записи истории, текущее — снапшот записи. Пусто (null), когда
 * содержимое не менялось или снапшотов нет (старые записи). */
function EntryDiff({ history, index }: { history: CapabilityHistoryEntry[]; index: number }) {
  const prev = index > 0 ? (history[index - 1].content ?? '') : '';
  const next = history[index].content ?? '';
  return <Diff oldText={prev} newText={next} />;
}

const CapabilityHistoryPage = observer(() => {
  const { id } = useParams<{ id: string }>();
  // Вид способности — из пути (/skills/:id/history, /commands/:id/history).
  const kind = useLocation().pathname.split('/')[1] as RouteKind;
  const [history, setHistory] = useState<CapabilityHistoryEntry[] | null>(null);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!kind || !id) return;
    setHistory(null);
    setError(false);
    http
      .get(`/${kind}/${id}/history`)
      .then((res) => setHistory(res.data))
      .catch(() => setError(true));
  }, [kind, id]);

  return (
    <div>
      <Link to={`/config/${kind}`} className="text-xs text-blue-600 hover:underline">
        ← {kind === 'skills' ? 'Skills' : 'Commands'}
      </Link>
      <Card>
        <CardTitle>История изменений</CardTitle>
        {error ? (
          <EmptyState>История не найдена</EmptyState>
        ) : history === null ? (
          <EmptyState>Загрузка…</EmptyState>
        ) : history.length === 0 ? (
          <EmptyState>Изменений пока нет</EmptyState>
        ) : (
          <div className="space-y-2">
            {history.map((entry, index) => (
              <div
                key={entry.id}
                className="rounded-md border border-slate-200 bg-slate-50 px-3 py-1.5 text-xs text-slate-600"
              >
                <div className="flex items-center gap-2">
                  <Badge variant="info">{ACTION_LABEL[entry.action]}</Badge>
                  <span className="font-semibold text-slate-700">{entry.actor_name}</span>
                  <span className="text-slate-400">
                    {formatTime(entry.created_at)}
                  </span>
                  {entry.detail && <span>→ {entry.detail}</span>}
                </div>
                <EntryDiff history={history} index={index} />
              </div>
            ))}
          </div>
        )}
      </Card>
    </div>
  );
});

export default CapabilityHistoryPage;

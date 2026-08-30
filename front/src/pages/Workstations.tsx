import { observer } from 'mobx-react-lite';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { EmptyState } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';
import type { Workstation } from '@/models/workstation';

function WsCard({ ws, store }: { ws: Workstation; store: ReturnType<typeof useStore> }) {
  const onRelease = async () => {
    try {
      await store.releaseWorkstation(ws.id);
    } catch (err) {
      if (err instanceof Error && err.message.includes('409')) {
        alert('На этом воркстейшне открыта сессия');
      } else {
        alert('Не удалось отпустить воркстейшн');
      }
    }
  };

  const onOccupy = async () => {
    try {
      await store.occupyWorkstation(ws.id);
    } catch (err) {
      if (err instanceof Error && err.message.includes('409')) {
        alert('На этом воркстейшне открыта сессия');
      } else {
        alert('Не удалось занять воркстейшн');
      }
    }
  };

  return (
    <Card key={ws.id}>
      <CardTitle>
        {ws.name}{' '}
        <Badge variant={ws.isReady ? 'ok' : 'warn'}>{ws.state}</Badge>
      </CardTitle>
      <CardMeta>{ws.isFree ? 'Свободен' : store.projectName(ws.project_id)}</CardMeta>
      <div className="mt-2 flex gap-2">
        {ws.isFree ? (
          <Button variant="outline" size="sm" onClick={onOccupy} disabled={store.activeProjectId === null}>
            Занять
          </Button>
        ) : (
          <Button variant="outline" size="sm" onClick={onRelease}>
            Отпустить
          </Button>
        )}
      </div>
    </Card>
  );
}

export const Workstations = observer(function Workstations() {
  const store = useStore();
  const active = store.activeProjectId;

  if (store.workstations.length === 0) {
    return <EmptyState>Воркстейшнов пока нет</EmptyState>;
  }

  if (active === null) {
    return (
      <div>
        <h3 className="mb-2 text-sm font-semibold text-slate-700">Все воркстейшны</h3>
        {store.workstations.map((ws) => (
          <WsCard key={ws.id} ws={ws} store={store} />
        ))}
      </div>
    );
  }

  const current = store.workstations.filter((ws) => ws.project_id === active);
  const others = store.workstations.filter((ws) => ws.project_id !== active);

  return (
    <div>
      <h3 className="mb-2 text-sm font-semibold text-slate-700">
        На текущем проекте ({current.length})
      </h3>
      {current.length === 0 && <EmptyState>На этом проекте воркстейшнов нет</EmptyState>}
      {current.map((ws) => (
        <WsCard key={ws.id} ws={ws} store={store} />
      ))}
      <h3 className="mb-2 mt-6 text-sm font-semibold text-slate-700">
        Другие проекты ({others.length})
      </h3>
      {others.length === 0 && <EmptyState>Воркстейшнов на других проектах нет</EmptyState>}
      {others.map((ws) => (
        <WsCard key={ws.id} ws={ws} store={store} />
      ))}
    </div>
  );
});
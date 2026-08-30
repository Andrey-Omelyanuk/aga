import { observer } from 'mobx-react-lite';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { EmptyState } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';

export const Workstations = observer(function Workstations() {
  const store = useStore();
  if (store.workstations.length === 0) {
    return <EmptyState>Воркстейшнов пока нет</EmptyState>;
  }
  return (
    <div>
      {store.workstations.map((ws) => (
        <Card key={ws.id}>
          <CardTitle>
            {ws.name}{' '}
            <Badge variant={ws.isReady ? 'ok' : 'warn'}>{ws.state}</Badge>
          </CardTitle>
          <CardMeta>Проект #{ws.project_id}</CardMeta>
        </Card>
      ))}
    </div>
  );
});
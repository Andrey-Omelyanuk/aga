import { observer } from 'mobx-react-lite';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { EmptyState } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';

export const Personnel = observer(function Personnel() {
  const store = useStore();
  if (store.users.length === 0) {
    return <EmptyState>Персонал загружается…</EmptyState>;
  }
  return (
    <div>
      {store.users.map((u) => (
        <Card key={u.id}>
          <CardTitle>
            {u.name} <Badge variant={u.isAgent ? 'info' : 'ok'}>{u.kind}</Badge>
          </CardTitle>
          <CardMeta>{u.is_super_user ? 'суперпользователь' : 'участник'}</CardMeta>
        </Card>
      ))}
    </div>
  );
});
import { observer } from 'mobx-react-lite';
import { Badge } from '@/components/ui/badge';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { EmptyState } from '@/components/ui/tabs';
import { Page } from '@/components/core/Page';
import { User } from '@/models/core';
import { useQuery } from '@/utils/mobx';

const PersonnelPage = observer(() => {
  const [users] = useQuery(User, { autoupdate: true });

  return (
    <Page queries={[users]}>
      {users.items.length === 0 && <EmptyState>Персонал пуст</EmptyState>}
      {users.items.map((u) => (
        <Card key={u.id}>
          <CardTitle>
            {u.name} <Badge variant={u.isAgent ? 'info' : 'ok'}>{u.kind}</Badge>
            {u.is_super_user && <Badge variant="warn">super</Badge>}
          </CardTitle>
          <CardMeta>#{u.id}</CardMeta>
        </Card>
      ))}
    </Page>
  );
});

export default PersonnelPage;
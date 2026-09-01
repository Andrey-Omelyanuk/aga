import { observer } from 'mobx-react-lite';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import me from '@/services/me';

const ProfilePage = observer(() => {
  const user = me.user;

  return (
    <div className="max-w-xl">
      <Card>
        <CardTitle>
          {user?.name} <Badge variant="info">{user?.kind}</Badge>
          {user?.is_super_user && <Badge variant="warn">admin</Badge>}
        </CardTitle>
        <CardMeta>#{user?.id}</CardMeta>
        <div className="mt-3">
          <Button variant="outline" size="sm" onClick={me.logout}>
            Выйти
          </Button>
        </div>
      </Card>
    </div>
  );
});

export default ProfilePage;

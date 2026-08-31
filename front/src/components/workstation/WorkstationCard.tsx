import { observer } from 'mobx-react-lite';
import { AxiosError } from 'axios';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Workstation } from '@/models/workstation';
import { toaster } from '@/utils/toaster';

export interface WorkstationCardProps {
  ws: Workstation;
  projectName: (id: number) => string;
  activeProjectId: number | null;
  onChanged: () => void;
}

function messageFor(e: unknown, action: string): string {
  const status = e instanceof AxiosError ? e.response?.status : undefined;
  if (status === 409) return 'На этом воркстейшне открыта сессия';
  if (status === 403) return 'Недостаточно прав';
  return `Не удалось ${action} воркстейшн`;
}

export const WorkstationCard = observer((props: WorkstationCardProps) => {
  const { ws, projectName, activeProjectId, onChanged } = props;

  const onOccupy = async () => {
    if (activeProjectId === null) return;
    try {
      await ws.action('switch', { project_id: activeProjectId });
    } catch (e) {
      toaster.show({ message: messageFor(e, 'занять'), intent: 'danger' });
    }
    onChanged();
  };

  const onRelease = async () => {
    try {
      await ws.action('release', {});
    } catch (e) {
      toaster.show({ message: messageFor(e, 'отпустить'), intent: 'danger' });
    }
    onChanged();
  };

  return (
    <Card key={ws.id}>
      <CardTitle>
        {ws.name} <Badge variant={ws.isReady ? 'ok' : 'warn'}>{ws.state}</Badge>
      </CardTitle>
      <CardMeta>{ws.isFree ? 'Свободен' : projectName(ws.project_id)}</CardMeta>
      <div className="mt-2 flex gap-2">
        {ws.isFree ? (
          <Button
            variant="outline"
            size="sm"
            onClick={onOccupy}
            disabled={activeProjectId === null}
          >
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
});
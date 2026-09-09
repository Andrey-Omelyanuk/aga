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
  onChanged: () => void;
}

function messageFor(e: unknown, action: string): string {
  const status = e instanceof AxiosError ? e.response?.status : undefined;
  // 409 может прийти от release: на станции открыта сессия (штатно отпускаем
  // только свободные станции).
  if (status === 409) return 'На этом воркстейшне открыта сессия';
  if (status === 403) return 'Недостаточно прав';
  return `Не удалось ${action} воркстейшн`;
}

export const WorkstationCard = observer((props: WorkstationCardProps) => {
  const { ws, projectName, onChanged } = props;

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
      {/* Свободная станция: «Отпустить» — штатная очистка/fsck пустой станции.
          Занятая сессией станция показывается только с именем проекта:
          release на ней вернёт 409, других действий API не предоставляет. */}
      {ws.isFree && (
        <div className="mt-2 flex gap-2">
          <Button variant="outline" size="sm" onClick={onRelease}>
            Отпустить
          </Button>
        </div>
      )}
    </Card>
  );
});

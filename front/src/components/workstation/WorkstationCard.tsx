import { observer } from 'mobx-react-lite';
import { AxiosError } from 'axios';
import { useTranslation } from 'react-i18next';
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

function messageFor(t: (key: string, options?: any) => string, e: unknown, action: string): string {
  const status = e instanceof AxiosError ? e.response?.status : undefined;
  // 409 может прийти от release: на станции открыта сессия (штатно отпускаем
  // только свободные станции).
  if (status === 409) return t('workstations.sessionOpen');
  if (status === 403) return t('workstations.insufficientPermissions');
  return t('workstations.failedToAction', { action });
}

export const WorkstationCard = observer((props: WorkstationCardProps) => {
  const { ws, projectName, onChanged } = props;
  const { t } = useTranslation();

  const onRelease = async () => {
    try {
      await ws.action('release', {});
    } catch (e) {
      toaster.show({ message: messageFor(t, e, 'release'), intent: 'danger' });
    }
    onChanged();
  };

  return (
    <Card key={ws.id}>
      <CardTitle>
        {ws.name} <Badge variant={ws.isReady ? 'ok' : 'warn'}>{ws.state}</Badge>
      </CardTitle>
      <CardMeta>{ws.isFree ? t('workstations.free') : projectName(ws.project_id)}</CardMeta>
      {/* Свободная станция: «Отпустить» — штатная очистка/fsck пустой станции.
          Занятая сессией станция показывается только с именем проекта:
          release на ней вернёт 409, других действий API не предоставляет. */}
      {ws.isFree && (
        <div className="mt-2 flex gap-2">
          <Button variant="outline" size="sm" onClick={onRelease}>
            {t('workstations.release')}
          </Button>
        </div>
      )}
    </Card>
  );
});

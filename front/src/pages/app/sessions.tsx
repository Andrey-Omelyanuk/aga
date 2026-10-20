import { observer } from 'mobx-react-lite';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  AND,
  ARRAY,
  EQ,
  IN,
  NUMBER,
  ObjectInput,
  STRING,
  Variable,
} from 'mobx-model-ui';
import { AxiosError } from 'axios';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { EmptyState } from '@/components/ui/tabs';
import { SelectInput, StringInput } from '@/components/core/inputs';
import { Page } from '@/components/core/Page';
import { Chat } from '@/models/chat';
import { Workstation } from '@/models/workstation';
import { useApp } from '@/store-hooks';
import { useInput, useObjectInput, useQuery, useQueryCacheSync } from '@/utils/mobx';
import { toaster } from '@/utils/toaster';

const SessionsPage = observer(() => {
  const { t } = useTranslation();
  const { activeProject } = useApp();
  const navigate = useNavigate();

  const [chats] = useQuery(Chat, { autoupdate: true });

  // Воркстейшны для открытия сессии: только ready и свободные (project_id = 0).
  // Станция, обслуживающая проект, уже несёт активную сессию — вторую открыть
  // нельзя (ядро ответит 409), поэтому занятые станции в выборку не берём.
  const stateFilter = useInput(() => new Variable(STRING(), { value: 'ready' }));
  const projectIds = useInput(() => new Variable(ARRAY(NUMBER()), { value: [0] }));

  const [sessionWs] = useQueryCacheSync(Workstation, {
    filter: AND(EQ('state', stateFilter), IN('project_id', projectIds)),
    autoupdate: true,
  });

  const wsInput = useObjectInput(
    () =>
      new ObjectInput(NUMBER(), {
        options: sessionWs,
      }),
    true,
  );
  const titleInput = useInput(() => new Variable(STRING(), { value: '' }));

  const open = async () => {
    const projectId = activeProject.value;
    if (projectId === undefined || projectId === null) {
      toaster.show({ message: t('sessions.selectProject'), intent: 'danger' });
      return;
    }
    const wsId = wsInput.value;
    if (wsId === undefined || wsId === null) {
      toaster.show({ message: t('sessions.selectWorkstation'), intent: 'danger' });
      return;
    }
    const ws = sessionWs.items.find((w) => w.id === Number(wsId));
    if (!ws) return;
    try {
      const chat = (await ws.action('session', {
        project_id: Number(projectId),
        title: titleInput.value || undefined,
      })) as Chat;
      titleInput.set('');
      wsInput.set(undefined);
      navigate(`/chat/${chat.id}`);
    } catch (e) {
      const status = e instanceof AxiosError ? e.response?.status : undefined;
      toaster.show({
        message:
          status === 409
            ? t('sessions.sessionAlreadyOpen')
            : t('sessions.failedToOpen'),
        intent: 'danger',
      });
    }
  };

  return (
    <Page queries={[sessionWs, chats]}>
      <div className="mb-4 flex items-center gap-2">
        <SelectInput
          input={wsInput}
          optionKey={(w) => String(w.id)}
          optionLabel={(w) => `${w.name} (${w.state})`}
          emptyLabel={t('sessions.selectWorkstationPlaceholder')}
        />
        <StringInput input={titleInput} placeholder={t('sessions.sessionName')} />
        <Button variant="secondary" onClick={open} disabled={sessionWs.items.length === 0}>
          {t('sessions.openSession')}
        </Button>
      </div>
      {sessionWs.items.length === 0 && (
        <p className="mb-3 text-sm text-slate-400">
          {t('sessions.noFreeWorkstations')}
        </p>
      )}
      {chats.items.length === 0 && <EmptyState>{t('sessions.noSessions')}</EmptyState>}
      {chats.items.map((chat) => (
        <Card key={chat.id}>
          <CardTitle>
            {chat.title || `${t('chat.sessionNumber')}${chat.id}`}{' '}
            <Badge variant={chat.isOpen ? 'ok' : 'warn'}>{chat.state}</Badge>
          </CardTitle>
          <CardMeta>
            {t('sessions.workstationNumber')}{chat.workstation_id ?? '—'}{t('sessions.createdBy')}{chat.created_by_id}
          </CardMeta>
          <div className="mt-1.5 flex gap-2">
            <Button variant="outline" size="sm" onClick={() => navigate(`/chat/${chat.id}`)}>
              {t('sessions.openInChat')}
            </Button>
            {chat.isOpen && (
              <Button
                variant="outline"
                size="sm"
                onClick={async () => {
                  await chat.action('close', {});
                  chats.shadowLoad();
                }}
              >
                {t('sessions.close')}
              </Button>
            )}
          </div>
        </Card>
      ))}
    </Page>
  );
});

export default SessionsPage;
import { observer } from 'mobx-react-lite';
import { useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
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
  const { activeProject } = useApp();
  const navigate = useNavigate();

  const [chats] = useQuery(Chat, { autoupdate: true });

  // Воркстейшны для открытия сессии: только ready и свободные или занятые
  // текущим проектом (тот же фильтр, что был в AppStore.sessionWorkstations).
  const stateFilter = useInput(() => new Variable(STRING(), { value: 'ready' }));
  const projectIds = useInput(() => new Variable(ARRAY(NUMBER()), { value: [0] }));
  useEffect(() => {
    const v = activeProject.value;
    projectIds.value = v === undefined || v === null ? [0] : [0, Number(v)];
  }, [activeProject.value]);

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
    const wsId = wsInput.value;
    if (wsId === undefined || wsId === null) {
      toaster.show({ message: 'Выберите готовый воркстейшн', intent: 'danger' });
      return;
    }
    const ws = sessionWs.items.find((w) => w.id === Number(wsId));
    if (!ws) return;
    try {
      const chat = (await ws.action('session', { title: titleInput.value || undefined })) as Chat;
      titleInput.set('');
      wsInput.set(undefined);
      navigate(`/chat/${chat.id}`);
    } catch (e) {
      const status = e instanceof AxiosError ? e.response?.status : undefined;
      toaster.show({
        message:
          status === 409
            ? 'На этом воркстейшне уже открыта сессия'
            : 'Не удалось открыть сессию',
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
          emptyLabel="Выберите воркстейшн..."
        />
        <StringInput input={titleInput} placeholder="Название сессии" />
        <Button variant="secondary" onClick={open} disabled={sessionWs.items.length === 0}>
          Открыть сессию
        </Button>
      </div>
      {sessionWs.items.length === 0 && (
        <p className="mb-3 text-sm text-slate-400">
          Нет готовых воркстейшнов — свободных или занятых текущим проектом
        </p>
      )}
      {chats.items.length === 0 && <EmptyState>Сессий пока нет</EmptyState>}
      {chats.items.map((chat) => (
        <Card key={chat.id}>
          <CardTitle>
            {chat.title || `Сессия #${chat.id}`}{' '}
            <Badge variant={chat.isOpen ? 'ok' : 'warn'}>{chat.state}</Badge>
          </CardTitle>
          <CardMeta>
            Воркстейшн #{chat.workstation_id ?? '—'} · создана #{chat.created_by_id}
          </CardMeta>
          <div className="mt-1.5 flex gap-2">
            <Button variant="outline" size="sm" onClick={() => navigate(`/chat/${chat.id}`)}>
              Открыть в чате
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
                Закрыть
              </Button>
            )}
          </div>
        </Card>
      ))}
    </Page>
  );
});

export default SessionsPage;
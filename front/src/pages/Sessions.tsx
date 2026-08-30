import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { EmptyState } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';

export const Sessions = observer(function Sessions() {
  const store = useStore();
  const [wsId, setWsId] = useState('');
  const [title, setTitle] = useState('');

  const open = async () => {
    if (!wsId) {
      alert('Выберите готовый воркстейшн');
      return;
    }
    try {
      await store.openWorkstationSession(Number(wsId), title);
      setTitle('');
    } catch (err) {
      if (err instanceof Error && err.message.includes('409')) {
        alert('На этом воркстейшне уже открыта сессия');
      } else {
        alert('Не удалось открыть сессию');
      }
    }
  };

  return (
    <div>
      <div className="mb-4 flex items-center gap-2">
        <Select value={wsId} onChange={(e) => setWsId(e.target.value)}>
          <option value="">Выберите воркстейшн...</option>
          {store.workstations.map((ws) => (
            <option key={ws.id} value={String(ws.id)} disabled={!ws.isReady}>
              {ws.name} ({ws.state})
            </option>
          ))}
        </Select>
        <Input placeholder="Название сессии" value={title} onChange={(e) => setTitle(e.target.value)} />
        <Button variant="secondary" onClick={open}>
          Открыть сессию
        </Button>
      </div>
      {store.chats.length === 0 && <EmptyState>Сессий пока нет</EmptyState>}
      {store.chats.map((chat) => (
        <Card key={chat.id}>
          <CardTitle>
            {chat.title || `Сессия #${chat.id}`}{' '}
            <Badge variant={chat.isOpen ? 'ok' : 'warn'}>{chat.state}</Badge>
          </CardTitle>
          <CardMeta>
            Воркстейшн #{chat.workstation_id ?? '—'} · создана #{chat.created_by_id}
          </CardMeta>
          <div className="mt-1.5 flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                store.setActiveTab('chat');
                void store.selectChat(chat.id);
              }}
            >
              Открыть в чате
            </Button>
            {chat.isOpen && (
              <Button variant="outline" size="sm" onClick={() => store.closeChat(chat.id)}>
                Закрыть
              </Button>
            )}
          </div>
        </Card>
      ))}
    </div>
  );
});
import { observer } from 'mobx-react-lite';
import { useEffect, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Select } from '@/components/ui/select';
import { EmptyState } from '@/components/ui/tabs';
import { ChatList } from '@/components/chat/ChatList';
import { MessageList } from '@/components/chat/MessageList';
import { Chat, loadChatDetail } from '@/models/chat';
import { AgentSet } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const POLL_INTERVAL_MS = 2000;

const ChatPage = observer(() => {
  const { id } = useParams();
  const navigate = useNavigate();
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState('');
  const [currentChat, setCurrentChat] = useState<Chat | null>(null);

  const chatId = id !== undefined && /^\d+$/.test(id) ? Number(id) : null;

  const [chats] = useQuery(Chat, { autoupdate: true });
  const [agentSets] = useQuery(AgentSet, { autoupdate: true });
  const seen = new Set<string>();
  const agents: string[] = [];
  for (const set of agentSets.items) {
    for (const agent of set.agents) {
      if (seen.has(agent.name)) continue;
      seen.add(agent.name);
      agents.push(agent.name);
    }
  }

  // Текущий чат: GET /chats/:id + опрос (pub-sub на бэкенде пока нет —
  // реактивные ответы агентов приходят асинхронно, перезагружаем по таймеру).
  useEffect(() => {
    if (chatId === null) {
      setCurrentChat(null);
      return;
    }
    let cancelled = false;
    const load = () =>
      loadChatDetail(chatId)
        .then((c) => {
          if (!cancelled) setCurrentChat(c);
        })
        .catch(() => {});
    void load();
    const timer = setInterval(() => void load(), POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [chatId]);

  useEffect(() => {
    if (inputRef.current) inputRef.current.focus();
  }, [chatId]);

  const send = async () => {
    const body = draft.trim();
    if (!body || chatId === null || !currentChat) return;
    setDraft('');
    await currentChat.action('messages', { body });
    const c = await loadChatDetail(chatId);
    setCurrentChat(c);
  };

  const createNew = async () => {
    const chat = new Chat({ title: 'Новая сессия' });
    await chat.create();
    navigate(`/chat/${chat.id}`);
  };

  return (
    <div className="-mx-5 -my-5 flex h-[calc(100vh-57px)]">
      <div className="flex w-72 flex-col border-r border-slate-200 bg-white">
        <div className="border-b border-slate-200 p-4">
          <Select>
            <option value="">Выберите агента…</option>
            {agents.map((a) => (
              <option key={a} value={a}>
                {a}
              </option>
            ))}
          </Select>
        </div>
        <div className="p-2.5">
          <Button variant="outline" className="w-full" onClick={createNew}>
            + Новая сессия
          </Button>
        </div>
        <ChatList
          chats={chats.items}
          currentId={chatId}
          onSelect={(cid) => navigate(`/chat/${cid}`)}
        />
      </div>

      <div className="flex flex-1 flex-col bg-white">
        <div className="border-b border-slate-200 px-5 py-3.5 font-semibold text-slate-800">
          {currentChat ? currentChat.title || `Сессия #${currentChat.id}` : 'Выберите сессию'}
        </div>
        <div className="flex-1 overflow-y-auto p-5">
          {!currentChat ? (
            <EmptyState>
              <div className="text-4xl">💬</div>
              <p>Выберите сессию или откройте новую</p>
            </EmptyState>
          ) : (
            <MessageList chat={currentChat} />
          )}
        </div>
        <div className="border-t border-slate-200 p-3.5">
          <div className="flex gap-2">
            <textarea
              ref={inputRef}
              className="min-h-[44px] flex-1 resize-none rounded-lg border border-slate-300 px-3 py-2.5 text-sm outline-none focus:border-blue-500"
              placeholder="Введите сообщение..."
              rows={1}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey) {
                  e.preventDefault();
                  void send();
                }
              }}
            />
            <Button onClick={send}>Отправить</Button>
          </div>
        </div>
      </div>
    </div>
  );
});

export default ChatPage;
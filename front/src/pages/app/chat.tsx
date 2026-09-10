import { observer } from 'mobx-react-lite';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { Button } from '@/components/ui/button';
import { Select } from '@/components/ui/select';
import { EmptyState } from '@/components/ui/tabs';
import { ChatList } from '@/components/chat/ChatList';
import { MessageList } from '@/components/chat/MessageList';
import { Chat, loadChatDetail } from '@/models/chat';
import { AgentSet, Shortcut } from '@/models/project';
import { useQuery } from '@/utils/mobx';
import pub_sub from '@/services/pub-sub';
import { cn } from '@/lib/utils';

// Начало последнего слова в тексте: всё после последнего пробельного символа
// (пробела или перевода строки). Сокращение подсказывается, когда текущее
// слово начинается с «/».
function lastWordStart(text: string): number {
  for (let i = text.length - 1; i >= 0; i--) {
    if (/\s/.test(text[i])) return i + 1;
  }
  return 0;
}

const ChatPage = observer(() => {
  const { id } = useParams();
  const navigate = useNavigate();
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState('');
  const [currentChat, setCurrentChat] = useState<Chat | null>(null);
  const currentChatRef = useRef<Chat | null>(null);
  currentChatRef.current = currentChat;

  const chatId = id !== undefined && /^\d+$/.test(id) ? Number(id) : null;

  const [chats] = useQuery(Chat, { autoupdate: true });
  const [agentSets] = useQuery(AgentSet, { autoupdate: true });
  const [shortcuts] = useQuery(Shortcut, { autoupdate: true });
  const seen = new Set<string>();
  const agents: string[] = [];
  for (const set of agentSets.items) {
    for (const agent of set.agents) {
      if (seen.has(agent.name)) continue;
      seen.add(agent.name);
      agents.push(agent.name);
    }
  }

  const loadDetail = useCallback(
    (id: number, onDone?: (c: Chat) => void) => {
      void loadChatDetail(id).then(onDone).catch(() => {});
    },
    [],
  );

  // Текущий чат: GET /chats/:id. Обновления — по websocket (centrifuge,
  // общий канал common): на событие нового сообщения перезагружаем деталь
  // открытого чата и список чатов. Опроса по таймеру больше нет.
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
    const unsubscribe = pub_sub.on_message((data: any) => {
      // Событие приходит и для самих нитей (их id — тоже чаты): перезагружаем
      // открытый чат, если сообщение появилось в нём или в одной из его нитей.
      const open = currentChatRef.current;
      const threadIds: number[] = [];
      if (open?.threads) {
        for (const t of open.threads) {
          threadIds.push(t.id);
          for (const nt of t.threads) threadIds.push(nt.id);
        }
      }
      if (data?.chat_id === chatId || threadIds.includes(data?.chat_id)) void load();
      void chats.load();
    });
    return () => {
      cancelled = true;
      unsubscribe();
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

  const reloadDetail = () => {
    if (chatId !== null) loadDetail(chatId, setCurrentChat);
  };

  // Автодополнение сокращений: при вводе «/» в начале текущего слова показываем
  // перечень сокращений, фильтруем по мере набора, Tab/Enter дополняет до
  // «/имя » (пробел вставляется сам). Escape прячет список, не трогая текст.
  const wordStart = lastWordStart(draft);
  const currentWord = draft.slice(wordStart);
  const queryPart = currentWord.startsWith('/') ? currentWord.slice(1) : null;
  const matches = useMemo(() => {
    if (queryPart === null) return [];
    const q = queryPart.toLowerCase();
    return shortcuts.items
      .filter((s) => s.name.toLowerCase().startsWith(q))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [queryPart, shortcuts.items]);

  const [dismissed, setDismissed] = useState(false);
  const [highlight, setHighlight] = useState(0);
  useEffect(() => {
    // Новое слово — сброс: список снова открыт, выбор на первом пункте.
    setDismissed(false);
    setHighlight(0);
  }, [queryPart]);

  const listOpen = !dismissed && queryPart !== null && matches.length > 0;
  const selected = matches.length > 0 ? matches[Math.min(highlight, matches.length - 1)] : null;

  const complete = (s: { id: number; name: string }) => {
    const value = `${draft.slice(0, wordStart)}/${s.name} `;
    setDraft(value);
    const el = inputRef.current;
    if (el) {
      // Каретку — в конец вставленного «/имя », дальше печатается текст.
      requestAnimationFrame(() => el.setSelectionRange(value.length, value.length));
      el.focus();
    }
  };

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (listOpen && e.key === 'Tab') {
      e.preventDefault();
      if (selected) complete(selected);
      return;
    }
    if (listOpen && e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      if (selected) complete(selected);
      return;
    }
    if (listOpen && e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlight((h) => (h + 1) % matches.length);
      return;
    }
    if (listOpen && e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => (h - 1 + matches.length) % matches.length);
      return;
    }
    if (listOpen && e.key === 'Escape') {
      e.preventDefault();
      setDismissed(true);
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
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
        <div className="flex items-center justify-between border-b border-slate-200 px-5 py-3.5">
          <div className="font-semibold text-slate-800">
            {currentChat ? currentChat.title || `Сессия #${currentChat.id}` : 'Выберите сессию'}
          </div>
          {currentChat && currentChat.workstation_id ? (
            <Link
              to={`/workstations/${currentChat.workstation_id}/changes`}
              className="text-xs text-blue-600 hover:underline"
            >
              Изменения
            </Link>
          ) : null}
        </div>
        <div className="flex-1 overflow-y-auto p-5">
          {!currentChat ? (
            <EmptyState>
              <div className="text-4xl">💬</div>
              <p>Выберите сессию или откройте новую</p>
            </EmptyState>
          ) : (
            <MessageList chat={currentChat} onChanged={reloadDetail} />
          )}
        </div>
        <div className="border-t border-slate-200 p-3.5">
          <div className="relative">
            {listOpen ? (
              <ul
                role="listbox"
                className="absolute bottom-full left-0 mb-1 max-h-48 w-full overflow-y-auto rounded-lg border border-slate-200 bg-white py-1 shadow-lg"
              >
                {matches.map((s, i) => (
                  <li
                    key={s.id}
                    role="option"
                    aria-selected={i === highlight}
                    className={cn(
                      'flex cursor-pointer gap-2 px-3 py-1.5 text-sm',
                      i === highlight && 'bg-slate-100',
                    )}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      complete(s);
                    }}
                    onMouseEnter={() => setHighlight(i)}
                  >
                    <span className="shrink-0 font-semibold text-slate-800">/{s.name}</span>
                    {s.content ? (
                      <span className="truncate text-xs text-slate-500">{s.content}</span>
                    ) : null}
                  </li>
                ))}
              </ul>
            ) : null}
            <div className="flex gap-2">
              <textarea
                ref={inputRef}
                className="min-h-[44px] flex-1 resize-none rounded-lg border border-slate-300 px-3 py-2.5 text-sm outline-none focus:border-blue-500"
                placeholder="Введите сообщение..."
                rows={1}
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={onInputKeyDown}
              />
              <Button onClick={send}>Отправить</Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
});

export default ChatPage;
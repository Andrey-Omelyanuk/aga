import { observer } from 'mobx-react-lite';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Select } from '@/components/ui/select';
import { EmptyState } from '@/components/ui/tabs';
import type { ChatMessage } from '@/models/chat';
import { store } from '@/store';
import { formatTime } from '@/lib/format';

export const Chat = observer(function Chat() {
  const s = store;
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const [draft, setDraft] = useState('');

  const send = async () => {
    const body = draft.trim();
    if (!body || !s.currentChatId) return;
    setDraft('');
    await s.sendMessage(body);
  };

  const createNew = async () => {
    const chat = await s.createChat();
    await s.selectChat(chat.id);
  };

  useEffect(() => {
    if (inputRef.current) inputRef.current.focus();
  }, [s.currentChatId]);

  return (
    <div className="-mx-5 -my-5 flex h-[calc(100vh-57px)]">
      <div className="flex w-72 flex-col border-r border-slate-200 bg-white">
        <div className="border-b border-slate-200 p-4">
          <Select>
            <option value="">Выберите агента…</option>
            {s.agents.map((a) => (
              <option key={a.name} value={a.name}>
                {a.name}
              </option>
            ))}
          </Select>
        </div>
        <div className="p-2.5">
          <Button variant="outline" className="w-full" onClick={createNew}>
            + Новая сессия
          </Button>
        </div>
        <div className="flex-1 overflow-y-auto p-2">
          {s.chats.map((chat) => (
            <button
              key={chat.id}
              className={`mb-1 w-full cursor-pointer rounded-md p-2.5 text-left ${
                s.currentChatId === chat.id ? 'bg-blue-100' : 'hover:bg-slate-100'
              }`}
              onClick={() => s.selectChat(chat.id)}
            >
              <div className="text-[13px] text-slate-800">
                {chat.title || `Сессия #${chat.id}`}
              </div>
              <div className="text-[11px] text-slate-400">{chat.state}</div>
            </button>
          ))}
        </div>
      </div>

      <div className="flex flex-1 flex-col bg-white">
        <div className="border-b border-slate-200 px-5 py-3.5 font-semibold text-slate-800">
          {s.currentChat ? s.currentChat.title || `Сессия #${s.currentChat.id}` : 'Выберите сессию'}
        </div>
        <div className="flex-1 overflow-y-auto p-5">
          {!s.currentChat ? (
            <EmptyState>
              <div className="text-4xl">💬</div>
              <p>Выберите сессию или откройте новую</p>
            </EmptyState>
          ) : s.currentChat.messages.length === 0 ? (
            <EmptyState>Сообщений пока нет</EmptyState>
          ) : (
            s.currentChat.messages.map((msg) => (
              <MessageView key={msg.id} message={msg} />
            ))
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

const MessageView = observer(function MessageView({ message }: { message: ChatMessage }) {
  const s = store;
  const chat = s.currentChat;
  const isUser = chat !== null && message.author_id === chat.id;
  const author = chat?.participantName(message.author_id) ?? `#${message.author_id}`;

  return (
    <div className={`mb-3 flex ${isUser ? 'justify-end' : 'justify-start'}`}>
      <div className="max-w-[70%]">
        <div
          className={`rounded-xl px-3.5 py-2.5 text-sm whitespace-pre-wrap break-words ${
            isUser ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-800'
          }`}
        >
          {message.body}
          {message.share_of_id ? ` · шар №${message.share_of_id}` : ''}
        </div>
        <div className="mt-0.5 text-[11px] text-slate-400">
          {author} · {formatTime(message.created_at)}
        </div>
        <Artifacts messageId={message.id} />
      </div>
    </div>
  );
});

function Artifacts({ messageId }: { messageId: number }) {
  const [items, setItems] = useState<Array<{ title?: string; kind?: string; content?: string }>>([]);
  useEffect(() => {
    void store.artifactsOf(messageId).then((data) => setItems((data as any[]) ?? []));
  }, [messageId]);
  if (items.length === 0) return null;
  return (
    <>
      {items.map((art, i) => (
        <div
          key={i}
          className="mt-1 rounded-lg border border-yellow-200 bg-yellow-50 px-3 py-1.5 text-xs whitespace-pre-wrap text-slate-700"
        >
          📎 {art.title || art.kind}: {art.content}
        </div>
      ))}
    </>
  );
}
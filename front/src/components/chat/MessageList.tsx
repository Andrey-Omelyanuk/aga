import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Chat, ChatMessage, ChatThread } from '@/models/chat';
import { formatTime } from '@/utils/dates';
import { Artifacts } from './Artifacts';
import { Button } from '@/components/ui/button';
import http from '@/services/http';

export interface MessageListProps {
  chat: Chat;
  /** Перезагрузить деталь чата после действия (начата нить, отправлено в
   *  нить или в родителя). */
  onChanged?: () => void;
}

/** Найти цепочку нитей (сверху вниз) до нити, начатой от сообщения originId.
 *  Возвращает id всех нитей-предков и самой нити — их надо раскрыть. */
function findThreadPath(threads: ChatThread[], originId: number): number[] | null {
  for (const t of threads) {
    if (t.start_message_id === originId) return [t.id];
    const nested = findThreadPath(t.threads, originId);
    if (nested) return [t.id, ...nested];
  }
  return null;
}

export const MessageList = observer((props: MessageListProps) => {
  const { chat, onChanged } = props;
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [startFor, setStartFor] = useState<number | null>(null);

  const toggle = (id: number) => {
    const next = new Set(expanded);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setExpanded(next);
  };

  const openThreadAt = (originId: number) => {
    document.getElementById(`msg-${originId}`)?.scrollIntoView?.({ block: 'center' });
    const path = findThreadPath(chat.threads, originId);
    if (path) {
      const next = new Set(expanded);
      for (const id of path) next.add(id);
      setExpanded(next);
    }
  };

  const reload = () => {
    if (onChanged) onChanged();
  };

  if (chat.messages.length === 0) {
    return <div className="py-16 text-center text-slate-400">Сообщений пока нет</div>;
  }
  return (
    <div>
      {chat.messages.map((msg) => (
        <div key={msg.id} id={`msg-${msg.id}`}>
          <MessageView
            chat={chat}
            message={msg}
            onStart={() => setStartFor(startFor === msg.id ? null : msg.id)}
            onOpenThread={() => (msg.thread_of_id ? openThreadAt(msg.thread_of_id) : undefined)}
          />
          {startFor === msg.id && (
            <StartThreadForm
              messageId={msg.id}
              onDone={() => {
                setStartFor(null);
                reload();
              }}
              onCancel={() => setStartFor(null)}
            />
          )}
          <Threads
            threads={chat.threads}
            originId={msg.id}
            expanded={expanded}
            onToggle={toggle}
            onChanged={reload}
          />
        </div>
      ))}
    </div>
  );
});

interface MessageViewProps {
  chat: Chat;
  message: ChatMessage;
  onStart: () => void;
  onOpenThread: () => void;
}

const MessageView = observer(({ chat, message, onStart, onOpenThread }: MessageViewProps) => {
  const isUser = message.author_id === chat.id;
  const author = chat.participantName(message.author_id);

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
        {message.thread_of_id ? (
          <button
            onClick={onOpenThread}
            className="mt-0.5 text-[11px] text-blue-600 hover:underline"
            title="Открыть нить"
          >
            из нити от сообщения #{message.thread_of_id}
          </button>
        ) : (
          <button
            onClick={onStart}
            className="mt-0.5 text-[11px] text-slate-400 hover:text-blue-600"
          >
            ↳ начать нить
          </button>
        )}
        <Artifacts messageId={message.id} />
      </div>
    </div>
  );
});

interface StartThreadFormProps {
  messageId: number;
  onDone: () => void;
  onCancel: () => void;
}

const StartThreadForm = observer(({ messageId, onDone, onCancel }: StartThreadFormProps) => {
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    if (!title.trim() || busy) return;
    setBusy(true);
    try {
      await http.post(`/messages/${messageId}/thread`, {
        title: title.trim(),
        body,
      });
      onDone();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mb-3 ml-6 rounded-lg border border-slate-200 bg-slate-50 p-2.5">
      <input
        className="mb-1.5 w-full rounded-md border border-slate-300 px-2.5 py-1.5 text-sm outline-none focus:border-blue-500"
        placeholder="Заголовок нити"
        value={title}
        onChange={(e) => setTitle(e.target.value)}
      />
      <textarea
        className="mb-1.5 min-h-[44px] w-full resize-none rounded-md border border-slate-300 px-2.5 py-1.5 text-sm outline-none focus:border-blue-500"
        placeholder="Первое сообщение нити"
        rows={2}
        value={body}
        onChange={(e) => setBody(e.target.value)}
      />
      <div className="flex gap-2">
        <Button size="sm" onClick={submit}>
          Начать
        </Button>
        <Button variant="ghost" size="sm" onClick={onCancel}>
          Отмена
        </Button>
      </div>
    </div>
  );
});

interface ThreadsProps {
  threads: ChatThread[];
  originId: number;
  expanded: Set<number>;
  onToggle: (id: number) => void;
  onChanged: () => void;
}

/** Начатые от сообщения originId нити, свёрнутые по умолчанию. */
const Threads = observer(({ threads, originId, expanded, onToggle, onChanged }: ThreadsProps) => {
  const items = threads.filter((t) => t.start_message_id === originId);
  if (items.length === 0) return null;
  return (
    <div className="mb-3 ml-6 border-l-2 border-slate-200 pl-3">
      {items.map((t) => (
        <ThreadEntry
          key={t.id}
          thread={t}
          open={expanded.has(t.id)}
          onToggle={() => onToggle(t.id)}
          onChanged={onChanged}
        />
      ))}
    </div>
  );
});

interface ThreadEntryProps {
  thread: ChatThread;
  open: boolean;
  onToggle: () => void;
  onChanged: () => void;
}

const ThreadEntry = observer(({ thread, open, onToggle, onChanged }: ThreadEntryProps) => {
  const title = thread.messages[0]?.title || `Нить #${thread.id}`;
  return (
    <div className="mb-2">
      <button
        onClick={onToggle}
        className="text-left text-xs text-slate-500 hover:text-slate-800"
        title={title}
      >
        {open ? '▾' : '▸'} {title}
      </button>
      {open && <ThreadBody thread={thread} onChanged={onChanged} />}
    </div>
  );
});

interface ThreadBodyProps {
  thread: ChatThread;
  onChanged: () => void;
}

/** Раскрытая нить: свои сообщения (с отправкой в родителя), ввод обычного
 *  сообщения и вложенные нити. */
const ThreadBody = observer(({ thread, onChanged }: ThreadBodyProps) => {
  const [draft, setDraft] = useState('');
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const send = async () => {
    const body = draft.trim();
    if (!body) return;
    setDraft('');
    await http.post(`/chats/${thread.id}/messages`, { body });
    onChanged();
  };

  const toParent = async (messageId: number) => {
    await http.post(`/messages/${messageId}/to-parent`, {});
    onChanged();
  };

  const toggle = (id: number) => {
    const next = new Set(expanded);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setExpanded(next);
  };

  return (
    <div className="mt-1 rounded-lg border border-slate-200 bg-white p-2.5">
      {thread.messages.map((m) => (
        <ThreadMessage
          key={m.id}
          thread={thread}
          message={m}
          onToParent={() => toParent(m.id)}
        />
      ))}
      <div className="mt-2 flex gap-2">
        <input
          className="min-h-[36px] flex-1 rounded-md border border-slate-300 px-2.5 py-1.5 text-sm outline-none focus:border-blue-500"
          placeholder="Сообщение в нить…"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void send();
          }}
        />
        <Button size="sm" onClick={send}>
          Отправить
        </Button>
      </div>
      {thread.messages.map((m) => (
        <Threads
          key={m.id}
          threads={thread.threads}
          originId={m.id}
          expanded={expanded}
          onToggle={toggle}
          onChanged={onChanged}
        />
      ))}
    </div>
  );
});

interface ThreadMessageProps {
  thread: ChatThread;
  message: ChatMessage;
  onToParent: () => void;
}

const ThreadMessage = observer(({ thread, message, onToParent }: ThreadMessageProps) => {
  const author = thread.participants.find((p) => p.id === message.author_id)?.name ?? `#${message.author_id}`;
  return (
    <div className="mb-2">
      <div className="rounded-lg bg-slate-100 px-3 py-2 text-sm whitespace-pre-wrap break-words text-slate-800">
        {message.title ? <div className="mb-0.5 font-semibold">{message.title}</div> : null}
        {message.body}
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-[11px] text-slate-400">
        <span>
          {author} · {formatTime(message.created_at)}
        </span>
        <button onClick={onToParent} className="text-slate-400 hover:text-blue-600">
          → в родителя
        </button>
      </div>
      <Artifacts messageId={message.id} />
    </div>
  );
});
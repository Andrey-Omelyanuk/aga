import { observer } from 'mobx-react-lite';
import { Chat, ChatMessage } from '@/models/chat';
import { formatTime } from '@/utils/dates';
import { Artifacts } from './Artifacts';

export interface MessageListProps {
  chat: Chat;
}

const MessageView = observer(({ chat, message }: { chat: Chat; message: ChatMessage }) => {
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
        <Artifacts messageId={message.id} />
      </div>
    </div>
  );
});

export const MessageList = observer((props: MessageListProps) => {
  const { chat } = props;
  if (chat.messages.length === 0) {
    return <div className="py-16 text-center text-slate-400">Сообщений пока нет</div>;
  }
  return (
    <div>
      {chat.messages.map((msg) => (
        <MessageView key={msg.id} chat={chat} message={msg} />
      ))}
    </div>
  );
});
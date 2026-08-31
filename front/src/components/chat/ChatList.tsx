import { observer } from 'mobx-react-lite';
import { Chat } from '@/models/chat';
import { cn } from '@/lib/utils';

export interface ChatListProps {
  chats: Chat[];
  currentId: number | null;
  onSelect: (id: number) => void;
}

export const ChatList = observer((props: ChatListProps) => {
  const { chats, currentId, onSelect } = props;

  return (
    <div className="flex-1 overflow-y-auto p-2">
      {chats.map((chat) => (
        <button
          key={chat.id}
          className={cn(
            'mb-1 w-full cursor-pointer rounded-md p-2.5 text-left',
            currentId === chat.id ? 'bg-blue-100' : 'hover:bg-slate-100',
          )}
          onClick={() => onSelect(chat.id)}
        >
          <div className="text-[13px] text-slate-800">
            {chat.title || `Сессия #${chat.id}`}
          </div>
          <div className="text-[11px] text-slate-400">{chat.state}</div>
        </button>
      ))}
    </div>
  );
});
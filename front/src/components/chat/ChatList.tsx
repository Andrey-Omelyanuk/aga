import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
import { Chat } from '@/models/chat';
import { cn } from '@/lib/utils';

export interface ChatListProps {
  chats: Chat[];
  currentId: number | null;
  onSelect: (id: number) => void;
}

export const ChatList = observer((props: ChatListProps) => {
  const { t } = useTranslation();
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
            {chat.title || `${t('chat.sessionNumber')}${chat.id}`}
          </div>
          <div className="text-[11px] text-slate-400">{chat.state}</div>
        </button>
      ))}
    </div>
  );
});
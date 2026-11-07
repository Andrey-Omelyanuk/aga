import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';
import http from '@/services/http';

export interface ChatMessage {
  id: number;
  body: string;
  author_id: number;
  created_at: string;
  share_of_id: number | null;
  /** Заголовок нити: живёт на первом сообщении нити. */
  title?: string | null;
  /** Копия сообщения нити в родительском чате: ссылка на сообщение, от
   *  которого нить началась. */
  thread_of_id?: number | null;
  /** Скрытая часть — заметка, по умолчанию свёрнутая. Заполняется
   *  сокращениями (`/имя`), вводом не задаётся. */
  hidden?: string | null;
  /** Источник: 'user' — набрано человеком, 'agent' — ответ агента от имени автора. */
  origin?: string;
}

export interface ChatParticipant {
  id: number;
  name: string;
  /** Вид участника: 'human' (человек) или 'agent'. Подсказка по «@» предлагает
   *  только людей. */
  kind?: string;
}

export interface ChatArtifact {
  title?: string;
  kind?: string;
  content?: string;
}

/** Нить — дочерний чат, привязанный к сообщению (start_message_id). Заголовок
 *  нити — title её первого сообщения. Разворачивается рекурсивно: своими
 *  сообщениями, участниками и вложенными нитями. */
export interface ChatThread {
  id: number;
  title: string | null;
  state: string;
  start_message_id: number | null;
  participants: ChatParticipant[];
  messages: ChatMessage[];
  threads: ChatThread[];
}

/** id всех нитей на любой глубине: событие из нити любого уровня
 *  перезагружает открытый чат (раньше смотрели только два уровня). */
export function collectThreadIds(threads: ChatThread[]): number[] {
  return threads.flatMap((t) => [t.id, ...collectThreadIds(t.threads ?? [])]);
}

function flattenThread(raw: any): ChatThread {
  return {
    ...raw.chat,
    messages: raw.messages ?? [],
    participants: raw.participants ?? [],
    threads: (raw.threads ?? []).map(flattenThread),
  };
}

@api('chats')
@model
export class Chat extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) title!: string;
  @field(STRING()) state!: string;
  @field(NUMBER()) workstation_id!: number | null;
  @field(NUMBER()) created_by_id!: number;
  @field() participants: ChatParticipant[] = [];
  @field() messages: ChatMessage[] = [];
  @field() threads: ChatThread[] = [];

  get isOpen(): boolean {
    return this.state === 'OPEN';
  }

  participantName(id: number): string {
    return this.participants.find((p) => p.id === id)?.name ?? `#${id}`;
  }
}

// GET /chats/:id возвращает обёртку {chat, messages, participants, threads}, а
// список (/chats) — плоские строки. Разворачиваем деталь в плоский объект,
// чтобы модель совпадала со списком (updateFromRaw не трогает отсутствующие
// поля, поэтому сообщения/участники переживают повторные загрузки списка).
export async function loadChatDetail(id: number): Promise<Chat> {
  const data = (await http.get(`/chats/${id}`)).data as {
    chat: Record<string, any>;
    messages: ChatMessage[];
    participants: ChatParticipant[];
    threads?: Array<Record<string, any>>;
  };
  const flat = {
    ...data.chat,
    messages: data.messages,
    participants: data.participants,
    threads: (data.threads ?? []).map(flattenThread),
  };
  return Chat.getModelDescriptor().updateCachedObject(flat) as Chat;
}
/** Операции с сообщениями, которых нет в CRUD модели `Chat`: компоненты ходят
 *  сюда, а не в `http` напрямую. */
export const messageApi = {
  /** Сообщение в чат (или нить); `parentId` — ответ на сообщение (так человек
   *  отвечает на вопрос агента `ask_human`). */
  send: (chatId: number, body: string, parentId?: number) =>
    http.post(`/chats/${chatId}/messages`, parentId != null ? { body, parent_id: parentId } : { body }),
  /** Новая нить от сообщения: заголовок и первое сообщение. */
  startThread: (messageId: number, title: string, body: string) =>
    http.post(`/messages/${messageId}/thread`, { title, body }),
  /** Копия сообщения нити в родительский чат (со ссылкой на источник). */
  toParent: (messageId: number) => http.post(`/messages/${messageId}/to-parent`, {}),
  artifacts: async (messageId: number) =>
    (await http.get<ChatArtifact[]>(`/messages/${messageId}/artifacts`)).data ?? [],
};

import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';
import http from '@/services/http';

export interface ChatMessage {
  id: number;
  body: string;
  author_id: number;
  created_at: string;
  share_of_id: number | null;
}

export interface ChatParticipant {
  id: number;
  name: string;
}

export interface ChatArtifact {
  title?: string;
  kind?: string;
  content?: string;
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

  get isOpen(): boolean {
    return this.state === 'OPEN';
  }

  participantName(id: number): string {
    return this.participants.find((p) => p.id === id)?.name ?? `#${id}`;
  }
}

// GET /chats/:id возвращает обёртку {chat, messages, participants}, а список
// (/chats) — плоские строки. Разворачиваем деталь в плоский объект, чтобы
// модель совпадала со списком (updateFromRaw не трогает отсутствующие поля,
// поэтому сообщения/участники переживают повторные загрузки списка).
export async function loadChatDetail(id: number): Promise<Chat> {
  const data = (await http.get(`/chats/${id}`)).data as {
    chat: Record<string, any>;
    messages: ChatMessage[];
    participants: ChatParticipant[];
  };
  const flat = { ...data.chat, messages: data.messages, participants: data.participants };
  return Chat.getModelDescriptor().updateCachedObject(flat) as Chat;
}
import { Model, model, id, field, NUMBER, STRING } from 'mobx-model-ui';

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

@model
export class Chat extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) title!: string;
  @field(STRING()) state!: string;
  @field() workstation_id: number | null = null;
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
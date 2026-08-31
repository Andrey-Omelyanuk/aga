import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

export interface Agent {
  name: string;
  description?: string;
  allowed_commands?: string[];
}

@api('agent-sets')
@model
export class AgentSet extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field() agents: Agent[] = [];
}
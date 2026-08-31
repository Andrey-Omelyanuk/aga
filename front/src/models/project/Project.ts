import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';
import type { Agent } from './AgentSet';

@api('projects')
@model
export class Project extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) git_url!: string;
  @field() agent_set: { id: number; name: string; agents: Agent[] } | null = null;

  get agents(): Agent[] {
    return this.agent_set?.agents ?? [];
  }
}
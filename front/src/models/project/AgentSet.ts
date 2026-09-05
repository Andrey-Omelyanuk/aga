import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

export interface AgentCapability {
  name: string;
}

export interface Territory {
  folder: string;
  excludes: string[];
}

export interface Agent {
  id?: number;
  name: string;
  description: string;
  tools: string[];
  max_iterations: number;
  /** Подключение к LLM (llm_connections); нет — дефолтная LLM из env. */
  llm_id?: number | null;
  parent_id?: number | null;
  /** Имя родителя в дереве (для сохранения; при загрузке выводится из parent_id). */
  parent?: string | null;
  skills: AgentCapability[];
  commands: AgentCapability[];
  territory: Territory;
}

export interface CatalogVersion {
  version: string;
  content: string;
}

export interface CatalogItem {
  id: number;
  name: string;
  content: string;
  deleted: boolean;
}

@api('agent-sets')
@model
export class AgentSet extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field() agents: Agent[] = [];
}
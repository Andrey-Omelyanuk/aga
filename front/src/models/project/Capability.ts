import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

/** Запись истории изменения скилла/команды: кто, когда и что сделал. */
export interface CapabilityHistoryEntry {
  id: number;
  action: 'create' | 'update' | 'rename' | 'delete';
  actor_id: number;
  actor_name: string;
  created_at: string;
  detail?: string | null;
}

/** Каталог способностей: скилл или команда с единственным текущим содержимым. */
abstract class Capability extends Model {
  abstract id: number;
  abstract name: string;
  abstract content: string;
  abstract deleted: boolean;
}

@api('skills')
@model
export class Skill extends Capability {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) content!: string;
  @field() deleted = false;
}

@api('commands')
@model
export class Command extends Capability {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) content!: string;
  @field() deleted = false;
}
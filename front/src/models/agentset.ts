import { Model, model, id, field, NUMBER, STRING } from 'mobx-model-ui';

@model
export class AgentSet extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field() agents: Array<{ name: string }> = [];
}
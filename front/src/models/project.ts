import { Model, model, id, field, NUMBER, STRING } from 'mobx-model-ui';

@model
export class Project extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) git_url!: string;
  @field() agent_set: any = null;

  get agents(): Array<{ name: string }> {
    return this.agent_set?.agents ?? [];
  }
}
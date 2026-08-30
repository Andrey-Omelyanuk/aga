import { Model, model, id, field, NUMBER, STRING } from 'mobx-model-ui';

@model
export class Workstation extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) state!: string;
  @field(NUMBER()) project_id!: number;

  get isReady(): boolean {
    return this.state === 'ready';
  }

  get isFree(): boolean {
    return this.project_id === 0;
  }
}
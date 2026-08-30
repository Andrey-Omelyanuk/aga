import { Model, model, id, field, NUMBER, STRING } from 'mobx-model-ui';

@model
export class User extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) kind!: string;
  @field() is_super_user: boolean = false;

  get isAgent(): boolean {
    return this.kind === 'agent';
  }
}
import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

/** Подключение к LLM: название, url API и ключ доступа. Агент набора
 *  ссылается на подключение (llm_id); без подключения агент работает на
 *  дефолтной LLM из env. Ключ отдаётся как есть, маскировки нет. */
@api('llms')
@model
export class Llm extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) api_url!: string;
  @field(STRING()) api_key?: string | null;
}
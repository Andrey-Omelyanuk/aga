import { Model, model, field, id, BOOLEAN, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

/** Подключение к LLM: название, url API, ключ доступа и модель (model_name —
 *  «model» зарезервировано базовым классом mobx-модели). Агент набора ссылается
 *  на подключение (llm_id); одно подключение — дефолтное (is_default): к нему
 *  ходят агенты без своего подключения. Ключ отдаётся как есть, без маскировки.
 *  Дефолтной LLM из env нет. */
@api('llms')
@model
export class Llm extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) api_url!: string;
  @field(STRING()) api_key?: string | null;
  @field(STRING()) model_name!: string;
  @field(BOOLEAN()) is_default!: boolean;
}
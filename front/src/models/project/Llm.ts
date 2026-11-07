import { Model, model, field, id, BOOLEAN, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

/** Подключение к LLM: название, url API, ключ доступа и модель (model_name —
 *  «model» зарезервировано базовым классом mobx-модели). Агент набора ссылается
 *  на подключение (llm_id); одно подключение — дефолтное (is_default): к нему
 *  ходят агенты без своего подключения. Ключ наружу не отдаётся — только признак
 *  has_api_key; задать/заменить ключ может админ (write-only поле в API).
 *  Дефолтной LLM из env нет. native_tools — режим инструментов агента:
 *  нативный function calling (true) или запасной текстовый — блоки ```bash. */
@api('llms')
@model
export class Llm extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) api_url!: string;
  @field(BOOLEAN()) has_api_key!: boolean;
  @field(STRING()) model_name!: string;
  @field(BOOLEAN()) is_default!: boolean;
  @field(BOOLEAN()) native_tools!: boolean;
}
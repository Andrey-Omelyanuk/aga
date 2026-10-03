import { Model, model, field, id, NUMBER, STRING } from 'mobx-model-ui';
import { api } from '@/services/http-adapter';

/** MCP-сервер из каталога: агенты набора получают его инструменты по имени.
 *  transport `http` — Streamable HTTP по url (ключ — Bearer); `stdio` —
 *  команда, запускаемая в воркстейшне агента (cwd — папка агента). */
@api('mcp-servers')
@model
export class McpServer extends Model {
  @id(NUMBER()) id!: number;
  @field(STRING()) name!: string;
  @field(STRING()) transport!: string;
  @field(STRING()) url!: string;
  @field(STRING()) command!: string;
  @field(STRING()) api_key?: string | null;
}

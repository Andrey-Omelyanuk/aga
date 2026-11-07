import http from '@/services/http';
import type { Agent, AgentSet } from './AgentSet';
import type { CapabilityHistoryEntry } from './Capability';

/** Операции API конфигурации платформы (LLM, MCP, наборы агентов, каталог
 *  способностей), которых нет в CRUD `@api`-моделей: компоненты ходят сюда, а
 *  не в `http` напрямую. */

/** Тело создания/правки подключения к LLM. `api_key`: нет поля — ключ не
 *  меняется, `''` — удалить (см. `keyPatch`). */
export interface LlmPayload {
  name: string;
  api_url: string;
  api_key?: string | null;
  model_name: string;
  native_tools: boolean;
  max_tokens?: number | null;
  temperature?: number | null;
}

export const llmApi = {
  create: (payload: LlmPayload) => http.post('/llms', payload),
  update: (id: number, payload: LlmPayload) => http.patch(`/llms/${id}`, payload),
  remove: (id: number) => http.delete(`/llms/${id}`),
  /** Дефолтная LLM: id подключения или null — снять выбор. */
  setDefault: (llmId: number | null) => http.post('/settings/llm-default', { llm_id: llmId }),
};

export interface McpPayload {
  name: string;
  transport: 'http' | 'stdio';
  url: string;
  command: string;
  api_key?: string | null;
}

export interface McpToolInfo {
  name: string;
  description: string;
}

export const mcpApi = {
  create: (payload: McpPayload) => http.post('/mcp-servers', payload),
  update: (id: number, payload: McpPayload) => http.patch(`/mcp-servers/${id}`, payload),
  remove: (id: number) => http.delete(`/mcp-servers/${id}`),
  /** Проверка http-сервера: подключение и список его инструментов. */
  tools: async (id: number) => (await http.get<McpToolInfo[]>(`/mcp-servers/${id}/tools`)).data,
};

export const agentSetApi = {
  create: (name: string) => http.post('/agent-sets', { name, agents: [] }),
  /** Состав набора заменяется целиком (PATCH). */
  update: (id: number, name: string, agents: Partial<Agent>[]) =>
    http.patch(`/agent-sets/${id}`, { name, agents }),
  remove: (set: Pick<AgentSet, 'id'>) => http.delete(`/agent-sets/${set.id}`),
};

/** Вид каталога: скиллы или сокращения (и путь API, и путь страницы). */
export type CatalogKind = 'skills' | 'shortcuts';

export const catalogApi = {
  create: (kind: CatalogKind, name: string) => http.post(`/${kind}`, { name, content: '' }),
  update: (kind: CatalogKind, id: number, name: string, content: string) =>
    http.patch(`/${kind}/${id}`, { name, content }),
  remove: (kind: CatalogKind, id: number) => http.delete(`/${kind}/${id}`),
  /** Список «Удалённые». */
  deleted: async <T>(kind: CatalogKind) => (await http.get<T[]>(`/${kind}?deleted=1`)).data,
  history: async (kind: CatalogKind, id: number) =>
    (await http.get<CapabilityHistoryEntry[]>(`/${kind}/${id}/history`)).data,
};

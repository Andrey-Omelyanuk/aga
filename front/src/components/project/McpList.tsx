import { Button } from '@/components/ui/button';
import { Card, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import { EmptyState } from '@/components/ui/tabs';
import type { McpServer } from '@/models/project';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import { observer } from 'mobx-react-lite';
import { useState } from 'react';

export interface McpListProps {
  servers: McpServer[];
  onChanged: () => void;
}

interface McpDraft {
  name: string;
  transport: 'http' | 'stdio';
  url: string;
  command: string;
  api_key: string;
}

interface McpToolInfo {
  name: string;
  description: string;
}

const emptyDraft = (): McpDraft => ({
  name: '',
  transport: 'http',
  url: '',
  command: '',
  api_key: '',
});
const fromServer = (s: McpServer): McpDraft => ({
  name: s.name,
  transport: s.transport === 'stdio' ? 'stdio' : 'http',
  url: s.url,
  command: s.command,
  api_key: s.api_key ?? '',
});

const isComplete = (d: McpDraft) =>
  d.name.trim() !== '' && (d.transport === 'http' ? d.url.trim() !== '' : d.command.trim() !== '');

const payload = (d: McpDraft) => ({
  name: d.name.trim(),
  transport: d.transport,
  url: d.transport === 'http' ? d.url.trim() : '',
  command: d.transport === 'stdio' ? d.command.trim() : '',
  api_key: d.transport === 'http' ? d.api_key.trim() || null : null,
});

/** Поля сервера: имя, транспорт и его адрес (url или команда), ключ — у http. */
const DraftFields = (props: { draft: McpDraft; onChange: (d: McpDraft) => void }) => {
  const { draft, onChange } = props;
  return (
    <>
      <Input
        placeholder="Название"
        value={draft.name}
        onChange={(e) => onChange({ ...draft, name: e.target.value })}
        className="max-w-40"
      />
      <Select
        className="max-w-28"
        value={draft.transport}
        onChange={(e) => onChange({ ...draft, transport: e.target.value as McpDraft['transport'] })}
        title="http — сервер по url; stdio — команда в воркстейшне агента"
      >
        <option value="http">http</option>
        <option value="stdio">stdio</option>
      </Select>
      {draft.transport === 'http' ? (
        <>
          <Input
            placeholder="URL (…/mcp)"
            value={draft.url}
            onChange={(e) => onChange({ ...draft, url: e.target.value })}
            className="max-w-64"
          />
          <Input
            placeholder="Ключ доступа (Bearer)"
            value={draft.api_key}
            onChange={(e) => onChange({ ...draft, api_key: e.target.value })}
            className="max-w-56"
          />
        </>
      ) : (
        <Input
          placeholder="Команда (npx -y @modelcontextprotocol/server-…)"
          value={draft.command}
          onChange={(e) => onChange({ ...draft, command: e.target.value })}
          className="max-w-md"
        />
      )}
    </>
  );
};

/** Каталог MCP-серверов: создание, правка, удаление и проверка http-сервера
 *  (подключение и список его инструментов). stdio-сервер запускается только в
 *  воркстейшне агента — отсюда не проверяется. */
export const McpList = observer((props: McpListProps) => {
  const { servers, onChanged } = props;
  const [draft, setDraft] = useState<McpDraft>(emptyDraft());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [edit, setEdit] = useState<McpDraft>(emptyDraft());
  const [checked, setChecked] = useState<Record<number, McpToolInfo[] | string>>({});

  const create = async () => {
    if (!isComplete(draft)) return;
    try {
      await http.post('/mcp-servers', payload(draft));
      toaster.show({ message: 'MCP-сервер добавлен', intent: 'success' });
      setDraft(emptyDraft());
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось добавить MCP-сервер (имя занято?)', intent: 'danger' });
    }
  };

  const save = async (id: number) => {
    if (!isComplete(edit)) return;
    try {
      await http.patch(`/mcp-servers/${id}`, payload(edit));
      toaster.show({ message: 'MCP-сервер сохранён', intent: 'success' });
      setEditingId(null);
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось сохранить MCP-сервер', intent: 'danger' });
    }
  };

  const remove = async (id: number) => {
    try {
      await http.delete(`/mcp-servers/${id}`);
      toaster.show({ message: 'MCP-сервер удалён', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось удалить MCP-сервер', intent: 'danger' });
    }
  };

  const check = async (id: number) => {
    try {
      const resp = await http.get(`/mcp-servers/${id}/tools`);
      setChecked((c) => ({ ...c, [id]: resp.data as McpToolInfo[] }));
    } catch (e) {
      const data = (e as { response?: { data?: unknown } }).response?.data;
      setChecked((c) => ({
        ...c,
        [id]: typeof data === 'string' && data ? data : 'сервер недоступен',
      }));
    }
  };

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <DraftFields draft={draft} onChange={setDraft} />
        <Button variant="secondary" onClick={create} disabled={!isComplete(draft)}>
          Добавить сервер
        </Button>
      </div>

      {servers.length === 0 ? (
        <EmptyState>MCP-серверов пока нет</EmptyState>
      ) : (
        servers.map((s) => {
          const result = checked[s.id];
          return (
            <Card key={s.id}>
              {editingId === s.id ? (
                <div className="flex flex-wrap items-center gap-2">
                  <DraftFields draft={edit} onChange={setEdit} />
                  <Button onClick={() => void save(s.id)}>Сохранить</Button>
                  <Button variant="ghost" onClick={() => setEditingId(null)}>
                    Отмена
                  </Button>
                </div>
              ) : (
                <>
                  <CardTitle>
                    {s.name}
                    <span className="ml-2 text-xs font-normal text-slate-500">· {s.transport}</span>
                  </CardTitle>
                  {s.transport === 'stdio' ? (
                    <div className="text-xs text-slate-500">Команда: {s.command}</div>
                  ) : (
                    <div className="text-xs text-slate-500">URL: {s.url}</div>
                  )}
                  {s.api_key != null && s.api_key !== '' && (
                    <div className="text-xs text-slate-500">Ключ: {s.api_key}</div>
                  )}
                  {typeof result === 'string' && (
                    <div className="mt-1 text-xs text-red-600">Ошибка: {result}</div>
                  )}
                  {Array.isArray(result) && (
                    <div className="mt-1 text-xs text-slate-600">
                      Инструменты ({result.length}):{' '}
                      {result.map((t) => (
                        <span key={t.name} className="mr-2 font-mono" title={t.description}>
                          {t.name}
                        </span>
                      ))}
                    </div>
                  )}
                  <div className="mt-2 flex gap-2">
                    {s.transport === 'http' && (
                      <Button variant="outline" size="sm" onClick={() => void check(s.id)}>
                        Проверить
                      </Button>
                    )}
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        setEditingId(s.id);
                        setEdit(fromServer(s));
                      }}
                    >
                      Править
                    </Button>
                    <Button variant="ghost" size="sm" onClick={() => void remove(s.id)}>
                      Удалить
                    </Button>
                  </div>
                </>
              )}
            </Card>
          );
        })
      )}
    </div>
  );
});

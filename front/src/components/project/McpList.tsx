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
import { useTranslation } from 'react-i18next';

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
  const { t } = useTranslation();
  const { draft, onChange } = props;
  return (
    <>
      <Input
        placeholder={t('config.name')}
        value={draft.name}
        onChange={(e) => onChange({ ...draft, name: e.target.value })}
        className="max-w-40"
      />
      <Select
        className="max-w-28"
        value={draft.transport}
        onChange={(e) => onChange({ ...draft, transport: e.target.value as McpDraft['transport'] })}
        title={`${t('config.httpTransport')}; ${t('config.stdioTransport')}`}
      >
        <option value="http">http</option>
        <option value="stdio">stdio</option>
      </Select>
      {draft.transport === 'http' ? (
        <>
          <Input
            placeholder={t('config.urlPlaceholder')}
            value={draft.url}
            onChange={(e) => onChange({ ...draft, url: e.target.value })}
            className="max-w-64"
          />
          <Input
            placeholder={t('config.accessKeyBearer')}
            value={draft.api_key}
            onChange={(e) => onChange({ ...draft, api_key: e.target.value })}
            className="max-w-56"
          />
        </>
      ) : (
        <Input
          placeholder={t('config.commandPlaceholder')}
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
  const { t } = useTranslation();
  const { servers, onChanged } = props;
  const [draft, setDraft] = useState<McpDraft>(emptyDraft());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [edit, setEdit] = useState<McpDraft>(emptyDraft());
  const [checked, setChecked] = useState<Record<number, McpToolInfo[] | string>>({});

  const create = async () => {
    if (!isComplete(draft)) return;
    try {
      await http.post('/mcp-servers', payload(draft));
      toaster.show({ message: t('config.mcpServerAdded'), intent: 'success' });
      setDraft(emptyDraft());
      onChanged();
    } catch {
      toaster.show({ message: t('config.mcpServerAddFailed'), intent: 'danger' });
    }
  };

  const save = async (id: number) => {
    if (!isComplete(edit)) return;
    try {
      await http.patch(`/mcp-servers/${id}`, payload(edit));
      toaster.show({ message: t('config.mcpServerSaved'), intent: 'success' });
      setEditingId(null);
      onChanged();
    } catch {
      toaster.show({ message: t('config.mcpServerSaveFailed'), intent: 'danger' });
    }
  };

  const remove = async (id: number) => {
    try {
      await http.delete(`/mcp-servers/${id}`);
      toaster.show({ message: t('config.mcpServerDeleted'), intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: t('config.mcpServerDeleteFailed'), intent: 'danger' });
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
        [id]: typeof data === 'string' && data ? data : t('config.serverUnavailable'),
      }));
    }
  };

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <DraftFields draft={draft} onChange={setDraft} />
        <Button variant="secondary" onClick={create} disabled={!isComplete(draft)}>
          {t('config.addServer')}
        </Button>
      </div>

      {servers.length === 0 ? (
        <EmptyState>{t('config.noMcpServers')}</EmptyState>
      ) : (
        servers.map((s) => {
          const result = checked[s.id];
          return (
            <Card key={s.id}>
              {editingId === s.id ? (
                <div className="flex flex-wrap items-center gap-2">
                  <DraftFields draft={edit} onChange={setEdit} />
                  <Button onClick={() => void save(s.id)}>{t('config.save')}</Button>
                  <Button variant="ghost" onClick={() => setEditingId(null)}>
                    {t('config.cancel')}
                  </Button>
                </div>
              ) : (
                <>
                  <CardTitle>
                    {s.name}
                    <span className="ml-2 text-xs font-normal text-slate-500">· {s.transport}</span>
                  </CardTitle>
                  {s.transport === 'stdio' ? (
                    <div className="text-xs text-slate-500">{t('config.command')} {s.command}</div>
                  ) : (
                    <div className="text-xs text-slate-500">{t('config.url')} {s.url}</div>
                  )}
                  {s.api_key != null && s.api_key !== '' && (
                    <div className="text-xs text-slate-500">{t('config.key')} {s.api_key}</div>
                  )}
                  {typeof result === 'string' && (
                    <div className="mt-1 text-xs text-red-600">{t('config.error')} {result}</div>
                  )}
                  {Array.isArray(result) && (
                    <div className="mt-1 text-xs text-slate-600">
                      {t('config.toolsCount')} ({result.length}):{' '}
                      {result.map((tool) => (
                        <span key={tool.name} className="mr-2 font-mono" title={tool.description}>
                          {tool.name}
                        </span>
                      ))}
                    </div>
                  )}
                  <div className="mt-2 flex gap-2">
                    {s.transport === 'http' && (
                      <Button variant="outline" size="sm" onClick={() => void check(s.id)}>
                        {t('config.check')}
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
                      {t('config.edit')}
                    </Button>
                    <Button variant="ghost" size="sm" onClick={() => void remove(s.id)}>
                      {t('config.delete')}
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

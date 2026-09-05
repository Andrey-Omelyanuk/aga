import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardTitle } from '@/components/ui/card';
import { EmptyState } from '@/components/ui/tabs';
import { Input } from '@/components/ui/input';
import { Select } from '@/components/ui/select';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import type { Llm } from '@/models/project';

export interface LlmListProps {
  connections: Llm[];
  onChanged: () => void;
}

interface LlmDraft {
  name: string;
  api_url: string;
  api_key: string;
  model_name: string;
}

const emptyDraft = (): LlmDraft => ({ name: '', api_url: '', api_key: '', model_name: '' });
const fromLlm = (c: Llm): LlmDraft => ({
  name: c.name,
  api_url: c.api_url,
  api_key: c.api_key ?? '',
  model_name: c.model_name,
});

/** Подключения к LLM: создание, правка (имя/url/ключ/модель), удаление и
 *  выбор дефолтной LLM (одно из подключений; к нему ходят агенты без своего).
 *  Ключ показывается как есть, маскировки нет. */
export const LlmList = observer((props: LlmListProps) => {
  const { connections, onChanged } = props;
  const [draft, setDraft] = useState<LlmDraft>(emptyDraft());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [edit, setEdit] = useState<LlmDraft>(emptyDraft());

  const defaultId = connections.find((c) => c.is_default)?.id ?? null;

  const create = async () => {
    if (!draft.name.trim() || !draft.api_url.trim() || !draft.model_name.trim()) return;
    try {
      await http.post('/llms', {
        name: draft.name.trim(),
        api_url: draft.api_url.trim(),
        api_key: draft.api_key.trim() || null,
        model_name: draft.model_name.trim(),
      });
      toaster.show({ message: 'Подключение создано', intent: 'success' });
      setDraft(emptyDraft());
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось создать подключение', intent: 'danger' });
    }
  };

  const save = async (id: number) => {
    if (!edit.name.trim() || !edit.api_url.trim() || !edit.model_name.trim()) return;
    try {
      await http.patch(`/llms/${id}`, {
        name: edit.name.trim(),
        api_url: edit.api_url.trim(),
        api_key: edit.api_key.trim() || null,
        model_name: edit.model_name.trim(),
      });
      toaster.show({ message: 'Подключение сохранено', intent: 'success' });
      setEditingId(null);
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось сохранить подключение', intent: 'danger' });
    }
  };

  const setDefault = async (llm_id: number | null) => {
    try {
      await http.post('/settings/llm-default', { llm_id });
      toaster.show({ message: 'Дефолтная LLM обновлена', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось обновить дефолтную LLM', intent: 'danger' });
    }
  };

  const remove = async (id: number) => {
    try {
      await http.delete(`/llms/${id}`);
      toaster.show({ message: 'Подключение удалено', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось удалить подключение', intent: 'danger' });
    }
  };

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <Input
          placeholder="Название"
          value={draft.name}
          onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))}
          className="max-w-40"
        />
        <Input
          placeholder="URL API (…/v1)"
          value={draft.api_url}
          onChange={(e) => setDraft((d) => ({ ...d, api_url: e.target.value }))}
          className="max-w-64"
        />
        <Input
          placeholder="Ключ доступа"
          value={draft.api_key}
          onChange={(e) => setDraft((d) => ({ ...d, api_key: e.target.value }))}
          className="max-w-56"
        />
        <Input
          placeholder="Модель"
          value={draft.model_name}
          onChange={(e) => setDraft((d) => ({ ...d, model: e.target.value }))}
          className="max-w-40"
        />
        <Button variant="secondary" onClick={create}>
          Создать подключение
        </Button>
      </div>

      <div className="mb-4 flex items-center gap-2">
        <span className="text-sm text-slate-500">Дефолтная LLM:</span>
        <Select
          className="max-w-56"
          value={defaultId != null ? String(defaultId) : ''}
          onChange={(e) => void setDefault(e.target.value ? Number(e.target.value) : null)}
          title="К ней ходят агенты без выбранного подключения"
        >
          <option value="">— нет —</option>
          {connections.map((c) => (
            <option key={c.id} value={String(c.id)}>
              {c.name}
            </option>
          ))}
        </Select>
      </div>

      {connections.length === 0 ? (
        <EmptyState>Подключений к LLM пока нет</EmptyState>
      ) : (
        connections.map((c) => (
          <Card key={c.id}>
            {editingId === c.id ? (
              <div className="flex flex-wrap items-center gap-2">
                <Input
                  value={edit.name}
                  onChange={(e) => setEdit((d) => ({ ...d, name: e.target.value }))}
                  className="max-w-40"
                />
                <Input
                  value={edit.api_url}
                  onChange={(e) => setEdit((d) => ({ ...d, api_url: e.target.value }))}
                  className="max-w-64"
                />
                <Input
                  value={edit.api_key}
                  onChange={(e) => setEdit((d) => ({ ...d, api_key: e.target.value }))}
                  className="max-w-56"
                />
                <Input
                  value={edit.model_name}
                  onChange={(e) => setEdit((d) => ({ ...d, model: e.target.value }))}
                  className="max-w-40"
                />
                <Button onClick={() => void save(c.id)}>Сохранить</Button>
                <Button variant="ghost" onClick={() => setEditingId(null)}>
                  Отмена
                </Button>
              </div>
            ) : (
              <>
                <CardTitle>
                  {c.name}
                  {c.is_default && (
                    <span className="ml-2 text-xs font-normal text-blue-600">
                      · дефолтная
                    </span>
                  )}
                </CardTitle>
                <div className="text-xs text-slate-500">URL: {c.api_url}</div>
                {c.api_key != null && c.api_key !== '' && (
                  <div className="text-xs text-slate-500">Ключ: {c.api_key}</div>
                )}
                <div className="text-xs text-slate-500">Модель: {c.model_name}</div>
                <div className="mt-2 flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => {
                      setEditingId(c.id);
                      setEdit(fromLlm(c));
                    }}
                  >
                    Править
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => void remove(c.id)}>
                    Удалить
                  </Button>
                </div>
              </>
            )}
          </Card>
        ))
      )}
    </div>
  );
});
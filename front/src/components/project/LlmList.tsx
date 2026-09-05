import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardTitle } from '@/components/ui/card';
import { EmptyState } from '@/components/ui/tabs';
import { Input } from '@/components/ui/input';
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
}

const emptyDraft = (): LlmDraft => ({ name: '', api_url: '', api_key: '' });
const fromLlm = (c: Llm): LlmDraft => ({
  name: c.name,
  api_url: c.api_url,
  api_key: c.api_key ?? '',
});

/** Список подключений к LLM: создание, правка (имя/url/ключ) и удаление.
 *  Ключ показывается как есть, маскировки нет — на него ссылаются агенты набора. */
export const LlmList = observer((props: LlmListProps) => {
  const { connections, onChanged } = props;
  const [draft, setDraft] = useState<LlmDraft>(emptyDraft());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [edit, setEdit] = useState<LlmDraft>(emptyDraft());

  const create = async () => {
    if (!draft.name.trim() || !draft.api_url.trim()) return;
    try {
      await http.post('/llms', {
        name: draft.name.trim(),
        api_url: draft.api_url.trim(),
        api_key: draft.api_key.trim() || null,
      });
      toaster.show({ message: 'Подключение создано', intent: 'success' });
      setDraft(emptyDraft());
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось создать подключение', intent: 'danger' });
    }
  };

  const save = async (id: number) => {
    if (!edit.name.trim() || !edit.api_url.trim()) return;
    try {
      await http.patch(`/llms/${id}`, {
        name: edit.name.trim(),
        api_url: edit.api_url.trim(),
        api_key: edit.api_key.trim() || null,
      });
      toaster.show({ message: 'Подключение сохранено', intent: 'success' });
      setEditingId(null);
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось сохранить подключение', intent: 'danger' });
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
        <Button variant="secondary" onClick={create}>
          Создать подключение
        </Button>
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
                <Button onClick={() => void save(c.id)}>Сохранить</Button>
                <Button variant="ghost" onClick={() => setEditingId(null)}>
                  Отмена
                </Button>
              </div>
            ) : (
              <>
                <CardTitle>{c.name}</CardTitle>
                <div className="text-xs text-slate-500">URL: {c.api_url}</div>
                {c.api_key != null && c.api_key !== '' && (
                  <div className="text-xs text-slate-500">Ключ: {c.api_key}</div>
                )}
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
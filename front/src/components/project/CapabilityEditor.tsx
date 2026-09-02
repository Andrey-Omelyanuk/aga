import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { EmptyState, Tabs } from '@/components/ui/tabs';
import { Link } from 'react-router-dom';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import { Markdown } from '@/components/core/Markdown';
import type { CatalogItem } from '@/models/project';

export type CapabilityKind = 'skills' | 'commands';

export interface CapabilityEditorProps {
  kind: CapabilityKind;
  items: CatalogItem[];
  deleted: CatalogItem[];
  onChanged: () => void;
}

const textareaClass =
  'min-h-[44px] w-full rounded-lg border border-slate-300 px-3 py-2 text-sm outline-none focus:border-blue-500';

interface CapabilityCardProps {
  kind: CapabilityKind;
  item: CatalogItem;
  disabled: boolean;
  onChanged: () => void;
}

const CapabilityCard = observer(({ kind, item, disabled, onChanged }: CapabilityCardProps) => {
  const [name, setName] = useState(item.name);
  const [content, setContent] = useState(item.content);
  const [mode, setMode] = useState<'edit' | 'view'>('edit');
  const [busy, setBusy] = useState(false);

  const save = async () => {
    const trimmedName = name.trim();
    if (!trimmedName) return;
    setBusy(true);
    try {
      await http.patch(`/${kind}/${item.id}`, { name: trimmedName, content });
      toaster.show({ message: 'Изменения сохранены', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось сохранить', intent: 'danger' });
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    setBusy(true);
    try {
      await http.delete(`/${kind}/${item.id}`);
      toaster.show({ message: 'Способность удалена', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось удалить', intent: 'danger' });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card>
      <div className="flex items-center gap-2">
        <Input
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="max-w-64"
        />
        <Button
          variant="outline"
          size="sm"
          disabled={busy || disabled || !name.trim()}
          onClick={() => void save()}
        >
          Сохранить
        </Button>
        <Button variant="ghost" size="sm" disabled={busy || disabled} onClick={() => void remove()}>
          Удалить
        </Button>
        <Link
          to={`/${kind}/${item.id}/history`}
          className="ml-auto text-xs text-blue-600 hover:underline"
        >
          История
        </Link>
      </div>
      <div className="mt-2">
        <div className="mb-2 flex items-center justify-between gap-2">
          <CardTitle>Содержимое</CardTitle>
          <Tabs
            tabs={[
              { value: 'edit', label: 'Правка' },
              { value: 'view', label: 'Просмотр' },
            ]}
            value={mode}
            onChange={setMode}
          />
        </div>
        {mode === 'view' ? (
          content.trim() ? (
            <Markdown content={content} />
          ) : (
            <EmptyState>Содержимое пусто</EmptyState>
          )
        ) : (
          <textarea
            className={textareaClass}
            placeholder="Содержимое скилла/команды (markdown; агент берёт его всегда)"
            value={content}
            onChange={(e) => setContent(e.target.value)}
          />
        )}
      </div>
    </Card>
  );
});

/** Редактор каталога способностей: у записи одно текущее содержимое,
 * правка перезаписывает его и пишется в историю; ниже — список «Удалённые»
 * (удалённые записи переживают в истории, их можно открыть). */
export const CapabilityEditor = observer(
  ({ kind, items, deleted, onChanged }: CapabilityEditorProps) => {
    const [name, setName] = useState('');
    const [content, setContent] = useState('');
    const [busy, setBusy] = useState(false);

    const create = async () => {
      const trimmed = name.trim();
      if (!trimmed) return;
      setBusy(true);
      try {
        await http.post(`/${kind}`, { name: trimmed, content });
        toaster.show({ message: 'Способность создана', intent: 'success' });
        setName('');
        setContent('');
        onChanged();
      } catch {
        toaster.show({ message: 'Не удалось создать способность', intent: 'danger' });
      } finally {
        setBusy(false);
      }
    };

    return (
      <div>
        <Card>
          <div className="mb-2 text-sm font-medium text-slate-700">Новая способность</div>
          <div className="flex items-center gap-2">
            <Input
              placeholder="Имя"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="max-w-64"
            />
            <Button variant="secondary" onClick={create} disabled={busy || !name.trim()}>
              Создать
            </Button>
          </div>
          <textarea
            className={textareaClass + ' mt-2'}
            placeholder="Содержимое (необязательно)"
            value={content}
            onChange={(e) => setContent(e.target.value)}
          />
        </Card>

        {items.length === 0 ? (
          <EmptyState>Каталог пуст</EmptyState>
        ) : (
          items.map((item) => (
            <CapabilityCard
              key={item.id}
              kind={kind}
              item={item}
              disabled={busy}
              onChanged={onChanged}
            />
          ))
        )}

        {deleted.length > 0 && (
          <Card>
            <CardTitle>Удалённые</CardTitle>
            <p className="mb-2 text-xs text-slate-400">
              Записи удалены, но их история сохранена.
            </p>
            <div className="space-y-1">
              {deleted.map((item) => (
                <div key={item.id} className="flex items-center gap-2 text-xs">
                  <span className="text-slate-500 line-through">{item.name}</span>
                  <Link
                    to={`/${kind}/${item.id}/history`}
                    className="text-blue-600 hover:underline"
                  >
                    История
                  </Link>
                </div>
              ))}
            </div>
          </Card>
        )}
      </div>
    );
  },
);

import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { EmptyState, Tabs } from '@/components/ui/tabs';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';
import { Link } from 'react-router-dom';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import { Markdown } from '@/components/core/Markdown';
import { cn } from '@/lib/utils';
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

const columnClass = 'h-[calc(100vh-9rem)] flex flex-col';

/** Список способностей слева: клик выбирает запись для правки справа; у каждой
 *  записи — удаление с подтверждением и переход к истории. */
interface CapabilityListProps {
  kind: CapabilityKind;
  items: CatalogItem[];
  selectedId: number | null;
  onSelect: (id: number) => void;
  onChanged: () => void;
}

const CapabilityList = observer(
  ({ kind, items, selectedId, onSelect, onChanged }: CapabilityListProps) => {
    const [name, setName] = useState('');
    const [busy, setBusy] = useState(false);
    const [confirming, setConfirming] = useState<CatalogItem | null>(null);

    const create = async () => {
      const trimmed = name.trim();
      if (!trimmed) return;
      setBusy(true);
      try {
        await http.post(`/${kind}`, { name: trimmed, content: '' });
        toaster.show({ message: 'Способность создана', intent: 'success' });
        setName('');
        onChanged();
      } catch {
        toaster.show({ message: 'Не удалось создать способность', intent: 'danger' });
      } finally {
        setBusy(false);
      }
    };

    const remove = async (item: CatalogItem) => {
      setConfirming(null);
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
      <div className={cn(columnClass, 'gap-3')}>
        <div>
          <div className="mb-2 text-sm font-medium text-slate-700">Новая способность</div>
          <div className="flex items-center gap-2">
            <Input
              placeholder="Имя"
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void create();
              }}
            />
            <Button
              variant="secondary"
              size="sm"
              onClick={create}
              disabled={busy || !name.trim()}
            >
              Создать
            </Button>
          </div>
        </div>

        {items.length === 0 ? (
          <EmptyState>Каталог пуст</EmptyState>
        ) : (
          <div className="flex-1 space-y-1 overflow-y-auto">
            {items.map((item) => (
              <div
                key={item.id}
                className={cn(
                  'flex items-center gap-1 rounded-md px-2 py-1.5 transition-colors',
                  selectedId === item.id
                    ? 'bg-blue-100'
                    : 'hover:bg-slate-100',
                )}
              >
                <button
                  onClick={() => onSelect(item.id)}
                  className={cn(
                    'min-w-0 flex-1 truncate rounded-md px-1 py-1 text-left text-sm',
                    selectedId === item.id
                      ? 'font-semibold text-blue-700'
                      : 'text-slate-700',
                  )}
                >
                  {item.name}
                </button>
                <Link
                  to={`/${kind}/${item.id}/history`}
                  title="История"
                  className="shrink-0 text-xs text-blue-600 hover:underline"
                >
                  История
                </Link>
                <Button
                  variant="ghost"
                  size="sm"
                  className="shrink-0"
                  disabled={busy}
                  onClick={() => setConfirming(item)}
                >
                  Удалить
                </Button>
              </div>
            ))}
          </div>
        )}

        {confirming && (
          <ConfirmDialog
            title="Удалить способность?"
            message={`«${confirming.name}» будет удалена, но её история сохранится.`}
            onConfirm={() => void remove(confirming)}
            onCancel={() => setConfirming(null)}
          />
        )}
      </div>
    );
  },
);

/** Редактор выбранной записи справа: правка/просмотр содержимого и сохранение. */
interface CapabilityDetailProps {
  kind: CapabilityKind;
  item: CatalogItem;
  onChanged: () => void;
}

const CapabilityDetail = observer(
  ({ kind, item, onChanged }: CapabilityDetailProps) => {
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

    return (
      <Card className={columnClass}>
        <div className="mb-3 flex items-center gap-2">
          <Input value={name} onChange={(e) => setName(e.target.value)} />
          <Button
            variant="outline"
            size="sm"
            disabled={busy || !name.trim()}
            onClick={() => void save()}
          >
            Сохранить
          </Button>
        </div>

        <div className="mb-2 flex items-center justify-between gap-2">
          <span className="text-sm font-medium text-slate-700">Содержимое</span>
          <Tabs
            tabs={[
              { value: 'edit', label: 'Правка' },
              { value: 'view', label: 'Просмотр' },
            ]}
            value={mode}
            onChange={setMode}
          />
        </div>
        <div className="flex-1 overflow-auto">
          {mode === 'view' ? (
            content.trim() ? (
              <Markdown content={content} />
            ) : (
              <EmptyState>Содержимое пусто</EmptyState>
            )
          ) : (
            <textarea
              className={cn(textareaClass, 'h-full min-h-[300px]')}
              placeholder="Содержимое скилла/команды (markdown; агент берёт его всегда)"
              value={content}
              onChange={(e) => setContent(e.target.value)}
            />
          )}
        </div>
      </Card>
    );
  },
);

/** Редактор каталога способностей: слева список записей (выбор, удаление с
 *  подтверждением, переход к истории), справа — редактор выбранной записи
 *  (правка/просмотр, сохранение). Внизу списка — «Удалённые». */
export const CapabilityEditor = observer(
  ({ kind, items, deleted, onChanged }: CapabilityEditorProps) => {
    const [selectedId, setSelectedId] = useState<number | null>(null);

    // Автовыбор: при пустом списке — ничего, иначе держим выбор валидным
    // (первая запись по умолчанию, после удаления выбранной — следующая).
    useEffect(() => {
      if (items.length === 0) {
        setSelectedId(null);
      } else if (selectedId == null || !items.some((item) => item.id === selectedId)) {
        setSelectedId(items[0].id);
      }
    }, [items, selectedId]);

    const selected = items.find((item) => item.id === selectedId) ?? null;

    return (
      <div className="grid grid-cols-1 gap-5 lg:grid-cols-[minmax(0,320px)_1fr]">
        <Card className={cn(columnClass, 'mb-0')}>
          <CapabilityList
            kind={kind}
            items={items}
            selectedId={selectedId}
            onSelect={setSelectedId}
            onChanged={onChanged}
          />
          {deleted.length > 0 && (
            <div className="mt-3 border-t border-slate-200 pt-3">
              <div className="mb-1 text-xs font-medium text-slate-500">Удалённые</div>
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
            </div>
          )}
        </Card>

        <div>
          {selected ? (
            <CapabilityDetail
              key={selected.id}
              kind={kind}
              item={selected}
              onChanged={onChanged}
            />
          ) : (
            <Card className={cn(columnClass, 'mb-0 items-center justify-center')}>
              <EmptyState>
                Выберите запись слева, чтобы просмотреть и отредактировать её
              </EmptyState>
            </Card>
          )}
        </div>
      </div>
    );
  },
);
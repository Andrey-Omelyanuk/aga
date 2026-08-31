import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import { EmptyState } from '@/components/ui/tabs';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import type { CatalogItem } from '@/models/project';

export type CapabilityKind = 'skills' | 'commands';

export interface CapabilityEditorProps {
  kind: CapabilityKind;
  items: CatalogItem[];
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
  const [version, setVersion] = useState('');
  const [content, setContent] = useState('');
  const [busy, setBusy] = useState(false);

  const rename = async (newName: string) => {
    const trimmed = newName.trim();
    if (!trimmed || trimmed === item.name) return;
    setBusy(true);
    try {
      await http.patch(`/${kind}/${item.id}`, { name: trimmed });
      toaster.show({ message: 'Имя обновлено', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось переименовать', intent: 'danger' });
    } finally {
      setBusy(false);
    }
  };

  const addVersion = async () => {
    const v = version.trim();
    const c = content.trim();
    if (!v || !c) return;
    setBusy(true);
    try {
      await http.post(`/${kind}/${item.id}/versions`, { version: v, content: c });
      toaster.show({ message: 'Версия добавлена', intent: 'success' });
      setVersion('');
      setContent('');
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось добавить версию', intent: 'danger' });
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
          defaultValue={item.name}
          key={item.name}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void rename(e.currentTarget.value);
          }}
          onBlur={(e) => {
            if (e.currentTarget.value.trim() !== item.name) void rename(e.currentTarget.value);
          }}
          className="max-w-64"
        />
        <Button variant="ghost" size="sm" disabled={busy || disabled} onClick={() => void remove()}>
          Удалить
        </Button>
      </div>
      <CardMeta className="mt-1">
        Версии:{' '}
        {item.versions.length === 0
          ? 'нет'
          : item.versions.map((v) => (
              <Badge key={v.version} variant="info">
                {v.version}
              </Badge>
            ))}
      </CardMeta>
      {item.versions.length > 0 && (
        <div className="mt-1.5 space-y-1">
          {item.versions.map((v) => (
            <div
              key={v.version}
              className="rounded-md border border-slate-200 bg-slate-50 px-3 py-1.5 text-xs text-slate-600"
            >
              <span className="font-semibold text-slate-700">v{v.version}</span> — {v.content}
            </div>
          ))}
        </div>
      )}
      <div className="mt-2">
        <CardTitle>Новая версия</CardTitle>
        <div className="flex items-center gap-2">
          <Input
            placeholder="Версия"
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            className="max-w-40"
          />
          <Button
            variant="outline"
            size="sm"
            disabled={busy || disabled || !version.trim() || !content.trim()}
            onClick={() => void addVersion()}
          >
            Добавить версию
          </Button>
        </div>
        <textarea
          className={textareaClass + ' mt-2'}
          placeholder="Содержимое версии"
          value={content}
          onChange={(e) => setContent(e.target.value)}
        />
      </div>
    </Card>
  );
});

/** Редактор каталога способностей: создание, переименование, версии
 * (добавление новой версии сохраняет предыдущие), удаление. */
export const CapabilityEditor = observer(({ kind, items, onChanged }: CapabilityEditorProps) => {
  const [name, setName] = useState('');
  const [version, setVersion] = useState('');
  const [content, setContent] = useState('');
  const [busy, setBusy] = useState(false);

  const create = async () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    try {
      const versions =
        version.trim() && content.trim()
          ? [{ version: version.trim(), content: content.trim() }]
          : [];
      await http.post(`/${kind}`, { name: trimmed, versions });
      toaster.show({ message: 'Способность создана', intent: 'success' });
      setName('');
      setVersion('');
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
          <Input
            placeholder="Версия (необязательно)"
            value={version}
            onChange={(e) => setVersion(e.target.value)}
            className="max-w-48"
          />
          <Button variant="secondary" onClick={create} disabled={busy || !name.trim()}>
            Создать
          </Button>
        </div>
        <textarea
          className={textareaClass + ' mt-2'}
          placeholder="Содержимое первой версии (необязательно)"
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
    </div>
  );
});
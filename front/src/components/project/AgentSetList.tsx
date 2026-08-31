import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { EmptyState } from '@/components/ui/tabs';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import http from '@/services/http';
import { toaster } from '@/utils/toaster';
import type { AgentSet } from '@/models/project';

export interface AgentSetListProps {
  sets: AgentSet[];
  selectedId: number | null;
  onSelect: (id: number) => void;
  onChanged: () => void;
}

export const AgentSetList = observer((props: AgentSetListProps) => {
  const { sets, selectedId, onSelect, onChanged } = props;
  const [name, setName] = useState('');

  const create = async () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    try {
      await http.post('/agent-sets', { name: trimmed, agents: [] });
      toaster.show({ message: 'Набор создан', intent: 'success' });
      setName('');
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось создать набор', intent: 'danger' });
    }
  };

  const remove = async (e: React.MouseEvent, set: AgentSet) => {
    e.stopPropagation();
    try {
      await http.delete(`/agent-sets/${set.id}`);
      toaster.show({ message: 'Набор удалён', intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: 'Не удалось удалить набор', intent: 'danger' });
    }
  };

  return (
    <div>
      <div className="mb-4 flex items-center gap-2">
        <Input
          placeholder="Имя нового набора"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void create();
          }}
        />
        <Button variant="secondary" onClick={create}>
          Создать набор
        </Button>
      </div>

      {sets.length === 0 ? (
        <EmptyState>Наборов пока нет</EmptyState>
      ) : (
        sets.map((set) => (
          <Card
            key={set.id}
            className={selectedId === set.id ? 'border-blue-500' : ''}
            onClick={() => onSelect(set.id)}
          >
            <CardTitle>{set.name}</CardTitle>
            <CardMeta>Агентов: {set.agents.length}</CardMeta>
            {set.agents.length > 0 && (
              <div className="mt-1.5 flex flex-wrap gap-1">
                {set.agents.map((a) => (
                  <Badge key={a.name} variant="info">
                    {a.name}
                  </Badge>
                ))}
              </div>
            )}
            <div className="mt-2 flex gap-2">
              <Button variant="outline" size="sm" onClick={() => onSelect(set.id)}>
                Редактировать
              </Button>
              <Button variant="ghost" size="sm" onClick={(e) => void remove(e, set)}>
                Удалить
              </Button>
            </div>
          </Card>
        ))
      )}
    </div>
  );
});
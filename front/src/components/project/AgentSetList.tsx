import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
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
  const { t } = useTranslation();
  const { sets, selectedId, onSelect, onChanged } = props;
  const [name, setName] = useState('');

  const create = async () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    try {
      await http.post('/agent-sets', { name: trimmed, agents: [] });
      toaster.show({ message: t('agentSets.created'), intent: 'success' });
      setName('');
      onChanged();
    } catch {
      toaster.show({ message: t('agentSets.createError'), intent: 'danger' });
    }
  };

  const remove = async (e: React.MouseEvent, set: AgentSet) => {
    e.stopPropagation();
    try {
      await http.delete(`/agent-sets/${set.id}`);
      toaster.show({ message: t('agentSets.deleted'), intent: 'success' });
      onChanged();
    } catch {
      toaster.show({ message: t('agentSets.deleteError'), intent: 'danger' });
    }
  };

  return (
    <div>
      <div className="mb-4 flex items-center gap-2">
        <Input
          placeholder={t('agentSets.newName')}
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void create();
          }}
        />
        <Button variant="secondary" onClick={create}>
          {t('agentSets.create')}
        </Button>
      </div>

      {sets.length === 0 ? (
        <EmptyState>{t('agentSets.empty')}</EmptyState>
      ) : (
        sets.map((set) => (
          <Card
            key={set.id}
            className={selectedId === set.id ? 'border-blue-500' : ''}
            onClick={() => onSelect(set.id)}
          >
            <CardTitle>{set.name}</CardTitle>
            <CardMeta>{t('agentSets.agentsCount', { count: set.agents.length })}</CardMeta>
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
                {t('common.edit')}
              </Button>
              <Button variant="ghost" size="sm" onClick={(e) => void remove(e, set)}>
                {t('common.delete')}
              </Button>
            </div>
          </Card>
        ))
      )}
    </div>
  );
});
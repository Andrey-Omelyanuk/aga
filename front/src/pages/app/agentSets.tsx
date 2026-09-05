import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Page } from '@/components/core/Page';
import { AgentSetList } from '@/components/project/AgentSetList';
import { AgentSetEditor } from '@/components/project/AgentSetEditor';
import { AgentSet, Command, Llm, Skill } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const AgentSetsPage = observer(() => {
  const [agentSets] = useQuery(AgentSet, { autoupdate: true });
  const [skills] = useQuery(Skill, { autoupdate: true });
  const [commands] = useQuery(Command, { autoupdate: true });
  const [connections] = useQuery(Llm, { autoupdate: true });
  const [selectedId, setSelectedId] = useState<number | null>(null);

  const reload = () => {
    agentSets.shadowLoad();
    skills.shadowLoad();
    commands.shadowLoad();
    connections.shadowLoad();
  };

  const selected = agentSets.items.find((s) => s.id === selectedId) ?? null;

  return (
    <Page queries={[agentSets, skills, commands, connections]}>
      <div className="grid grid-cols-1 gap-5 lg:grid-cols-[minmax(0,380px)_1fr]">
        <AgentSetList
          sets={agentSets.items}
          selectedId={selectedId}
          onSelect={setSelectedId}
          onChanged={reload}
        />
        <div>
          {selected ? (
            <AgentSetEditor
              key={selected.id}
              setId={selected.id}
              name={selected.name}
              agents={selected.agents}
              skills={skills.items}
              commands={commands.items}
              connections={connections.items}
              onSaved={reload}
            />
          ) : (
            <div className="p-10 text-slate-400">
              Выберите набор слева, чтобы посмотреть и отредактировать его состав
            </div>
          )}
        </div>
      </div>
    </Page>
  );
});

export default AgentSetsPage;
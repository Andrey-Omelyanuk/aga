import { observer } from 'mobx-react-lite';
import { useState } from 'react';
import { Page } from '@/components/core/Page';
import { Tabs } from '@/components/ui/tabs';
import {
  CapabilityEditor,
  type CapabilityKind,
} from '@/components/project/CapabilityEditor';
import { Command, Skill } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const CapabilitiesPage = observer(() => {
  const [skills] = useQuery(Skill, { autoupdate: true });
  const [commands] = useQuery(Command, { autoupdate: true });
  const [kind, setKind] = useState<CapabilityKind>('skills');

  const reload = () => {
    skills.shadowLoad();
    commands.shadowLoad();
  };

  const items = kind === 'skills' ? skills.items : commands.items;

  return (
    <Page queries={[skills, commands]}>
      <Tabs
        tabs={[
          { value: 'skills', label: 'Скиллы' },
          { value: 'commands', label: 'Команды' },
        ]}
        value={kind}
        onChange={setKind}
      />
      <div className="mt-4">
        <CapabilityEditor kind={kind} items={items} onChanged={reload} />
      </div>
    </Page>
  );
});

export default CapabilitiesPage;
import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { Page } from '@/components/core/Page';
import { Tabs } from '@/components/ui/tabs';
import {
  CapabilityEditor,
  type CapabilityKind,
} from '@/components/project/CapabilityEditor';
import { Command, Skill } from '@/models/project';
import { useQuery } from '@/utils/mobx';
import http from '@/services/http';
import type { CatalogItem } from '@/models/project';

const CapabilitiesPage = observer(() => {
  const [skills] = useQuery(Skill, { autoupdate: true });
  const [commands] = useQuery(Command, { autoupdate: true });
  const [kind, setKind] = useState<CapabilityKind>('skills');
  const [deleted, setDeleted] = useState<Record<CapabilityKind, CatalogItem[]>>({
    skills: [],
    commands: [],
  });

  const reloadDeleted = async () => {
    for (const k of ['skills', 'commands'] as CapabilityKind[]) {
      try {
        const res = await http.get(`/${k}?deleted=1`);
        setDeleted((prev) => ({ ...prev, [k]: res.data }));
      } catch {
        // список «Удалённых» не критичен для основной работы
      }
    }
  };

  useEffect(() => {
    void reloadDeleted();
  }, [kind]);

  const reload = () => {
    skills.shadowLoad();
    commands.shadowLoad();
    void reloadDeleted();
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
        <CapabilityEditor
          kind={kind}
          items={items}
          deleted={deleted[kind]}
          onChanged={reload}
        />
      </div>
    </Page>
  );
});

export default CapabilitiesPage;

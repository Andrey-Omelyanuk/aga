import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { Page } from '@/components/core/Page';
import {
  CapabilityEditor,
  type CapabilityKind,
} from '@/components/project/CapabilityEditor';
import { Command, Skill } from '@/models/project';
import type { CatalogItem } from '@/models/project';
import { useQuery } from '@/utils/mobx';
import http from '@/services/http';

/** Страницы каталога под Config: вид способности (skills/commands) — из пути
 * (/config/skills, /config/commands), как в capabilityHistory. */
const ConfigCapabilitiesPage = observer(() => {
  const kind = useLocation().pathname.split('/')[2] as CapabilityKind;
  const isSkills = kind === 'skills';
  const [skills] = useQuery(Skill, { autoupdate: true });
  const [commands] = useQuery(Command, { autoupdate: true });
  const [deleted, setDeleted] = useState<CatalogItem[]>([]);

  const reloadDeleted = async () => {
    try {
      const res = await http.get(`/${kind}?deleted=1`);
      setDeleted(res.data);
    } catch {
      // список «Удалённых» не критичен для основной работы
    }
  };

  useEffect(() => {
    setDeleted([]);
    void reloadDeleted();
  }, [kind]);

  const reload = () => {
    skills.shadowLoad();
    commands.shadowLoad();
    void reloadDeleted();
  };

  const items = isSkills ? skills.items : commands.items;

  return (
    <Page queries={isSkills ? [skills] : [commands]}>
      <div className="mt-4">
        <CapabilityEditor
          kind={kind}
          items={items}
          deleted={deleted}
          onChanged={reload}
        />
      </div>
    </Page>
  );
});

export default ConfigCapabilitiesPage;
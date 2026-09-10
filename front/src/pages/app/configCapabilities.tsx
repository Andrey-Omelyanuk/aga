import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { Page } from '@/components/core/Page';
import {
  CapabilityEditor,
  type CapabilityKind,
} from '@/components/project/CapabilityEditor';
import { Command, Shortcut, Skill } from '@/models/project';
import type { CatalogItem } from '@/models/project';
import { useQuery } from '@/utils/mobx';
import http from '@/services/http';

/** Страницы каталога под Config: вид записи (skills/commands/shortcuts) — из
 * пути (/config/skills, /config/commands, /config/shortcuts), как в
 * capabilityHistory. */
const ConfigCapabilitiesPage = observer(() => {
  const kind = useLocation().pathname.split('/')[2] as CapabilityKind;
  const [skills] = useQuery(Skill, { autoupdate: true });
  const [commands] = useQuery(Command, { autoupdate: true });
  const [shortcuts] = useQuery(Shortcut, { autoupdate: true });
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
    shortcuts.shadowLoad();
    void reloadDeleted();
  };

  const query =
    kind === 'shortcuts' ? shortcuts : kind === 'commands' ? commands : skills;

  return (
    <Page queries={[query]}>
      <div className="mt-4">
        <CapabilityEditor
          kind={kind}
          items={query.items}
          deleted={deleted}
          onChanged={reload}
        />
      </div>
    </Page>
  );
});

export default ConfigCapabilitiesPage;

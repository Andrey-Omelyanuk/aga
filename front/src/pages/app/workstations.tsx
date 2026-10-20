import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
import { Page } from '@/components/core/Page';
import { WorkstationList } from '@/components/workstation/WorkstationList';
import { Workstation } from '@/models/workstation';
import { useApp } from '@/store-hooks';
import { useQuery } from '@/utils/mobx';

const WorkstationsPage = observer(() => {
  const { t } = useTranslation();
  const { activeProject } = useApp();
  const [workstations] = useQuery(Workstation, { autoupdate: true });

  const projects = activeProject.options.items;
  const projectName = (id: number): string => {
    const p = projects.find((x) => x.id === id);
    return p ? p.git_url : `${t('workstations.projectNumber')}${id}`;
  };
  const activeId =
    activeProject.value !== undefined && activeProject.value !== null
      ? Number(activeProject.value)
      : null;
  const reload = () => workstations.shadowLoad();

  return (
    <Page queries={[workstations]}>
      {activeId === null ? (
        <WorkstationList
          title={t('workstations.allWorkstations')}
          workstations={workstations.items}
          projectName={projectName}
          onChanged={reload}
        />
      ) : (
        <>
          {/* project_id станции — производное от активной сессии: станция
              обслуживает проект, пока сессия открыта. */}
          <WorkstationList
            title={t('workstations.onCurrentProject')}
            workstations={workstations.items.filter((ws) => ws.project_id === activeId)}
            projectName={projectName}
            onChanged={reload}
          />
          <div className="mt-6" />
          <WorkstationList
            title={t('workstations.otherProjects')}
            workstations={workstations.items.filter((ws) => ws.project_id !== activeId)}
            projectName={projectName}
            onChanged={reload}
          />
        </>
      )}
    </Page>
  );
});

export default WorkstationsPage;
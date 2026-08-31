import { observer } from 'mobx-react-lite';
import { Page } from '@/components/core/Page';
import { WorkstationList } from '@/components/workstation/WorkstationList';
import { Workstation } from '@/models/workstation';
import { useApp } from '@/store-hooks';
import { useQuery } from '@/utils/mobx';

const WorkstationsPage = observer(() => {
  const { activeProject } = useApp();
  const [workstations] = useQuery(Workstation, { autoupdate: true });

  const projects = activeProject.options.items;
  const projectName = (id: number): string => {
    const p = projects.find((x) => x.id === id);
    return p ? p.git_url : `Проект #${id}`;
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
          title="Все воркстейшны"
          workstations={workstations.items}
          projectName={projectName}
          activeProjectId={null}
          onChanged={reload}
        />
      ) : (
        <>
          <WorkstationList
            title="На текущем проекте"
            workstations={workstations.items.filter((ws) => ws.project_id === activeId)}
            projectName={projectName}
            activeProjectId={activeId}
            onChanged={reload}
          />
          <div className="mt-6" />
          <WorkstationList
            title="Другие проекты"
            workstations={workstations.items.filter((ws) => ws.project_id !== activeId)}
            projectName={projectName}
            activeProjectId={activeId}
            onChanged={reload}
          />
        </>
      )}
    </Page>
  );
});

export default WorkstationsPage;
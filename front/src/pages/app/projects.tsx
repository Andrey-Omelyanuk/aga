import { observer } from 'mobx-react-lite';
import { Page } from '@/components/core/Page';
import { ProjectCreate } from '@/components/project/ProjectCreate';
import { ProjectList } from '@/components/project/ProjectList';
import { Project } from '@/models/project';
import { useQuery } from '@/utils/mobx';

const ProjectsPage = observer(() => {
  const [projects] = useQuery(Project, { autoupdate: true });
  const reload = () => projects.shadowLoad();

  return (
    <Page queries={[projects]}>
      <ProjectCreate onCreated={reload} />
      <ProjectList projects={projects.items} onDeleted={reload} />
    </Page>
  );
});

export default ProjectsPage;
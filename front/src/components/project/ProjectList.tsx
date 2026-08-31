import { observer } from 'mobx-react-lite';
import { Badge } from '@/components/ui/badge';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { DeleteObjectButton } from '@/components/core/inputs';
import { EmptyState } from '@/components/ui/tabs';
import { Project } from '@/models/project';

export interface ProjectListProps {
  projects: Project[];
  onDeleted?: () => void;
}

export const ProjectList = observer((props: ProjectListProps) => {
  const { projects, onDeleted } = props;

  if (projects.length === 0) {
    return <EmptyState>Проектов пока нет</EmptyState>;
  }

  return (
    <div>
      {projects.map((p) => (
        <Card key={p.id}>
          <CardTitle>{p.git_url}</CardTitle>
          <CardMeta>Проект #{p.id}</CardMeta>
          {p.agents.length > 0 && (
            <div className="mt-1.5">
              {p.agents.map((a) => (
                <Badge key={a.name} variant="info">
                  {a.name}
                </Badge>
              ))}
            </div>
          )}
          <div className="mt-2">
            <DeleteObjectButton obj={p} onDeleted={onDeleted} />
          </div>
        </Card>
      ))}
    </div>
  );
});
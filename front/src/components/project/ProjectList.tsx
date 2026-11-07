import { observer } from 'mobx-react-lite';
import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { Card, CardMeta, CardTitle } from '@/components/ui/card';
import { DeleteObjectButton } from '@/components/core/inputs';
import { EmptyState } from '@/components/ui/tabs';
import { Project } from '@/models/project';

export interface ProjectListProps {
  projects: Project[];
  onDeleted?: () => void;
  /** Удалять проекты может только админ. */
  canDelete?: boolean;
}

export const ProjectList = observer((props: ProjectListProps) => {
  const { t } = useTranslation();
  const { projects, onDeleted, canDelete = false } = props;

  if (projects.length === 0) {
    return <EmptyState>{t('projects.empty')}</EmptyState>;
  }

  return (
    <div>
      {projects.map((p) => (
        <Card key={p.id}>
          <CardTitle>{p.git_url}</CardTitle>
          <CardMeta>{t('projects.projectNumber')}{p.id}</CardMeta>
          {p.agents.length > 0 && (
            <div className="mt-1.5">
              {p.agents.map((a) => (
                <Badge key={a.name} variant="info">
                  {a.name}
                </Badge>
              ))}
            </div>
          )}
          {canDelete && (
            <div className="mt-2">
              <DeleteObjectButton obj={p} onDeleted={onDeleted} />
            </div>
          )}
        </Card>
      ))}
    </div>
  );
});
import { observer } from 'mobx-react-lite';
import { EmptyState } from '@/components/ui/tabs';
import { Workstation } from '@/models/workstation';
import { WorkstationCard } from './WorkstationCard';

export interface WorkstationListProps {
  title: string;
  workstations: Workstation[];
  projectName: (id: number) => string;
  activeProjectId: number | null;
  onChanged: () => void;
}

export const WorkstationList = observer((props: WorkstationListProps) => {
  const { title, workstations, projectName, activeProjectId, onChanged } = props;

  return (
    <div>
      <h3 className="mb-2 text-sm font-semibold text-slate-700">
        {title} ({workstations.length})
      </h3>
      {workstations.length === 0 && <EmptyState>Воркстейшнов нет</EmptyState>}
      {workstations.map((ws) => (
        <WorkstationCard
          key={ws.id}
          ws={ws}
          projectName={projectName}
          activeProjectId={activeProjectId}
          onChanged={onChanged}
        />
      ))}
    </div>
  );
});
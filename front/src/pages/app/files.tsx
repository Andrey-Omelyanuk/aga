import { observer } from 'mobx-react-lite';
import { Select } from '@/components/ui/select';
import { Page } from '@/components/core/Page';
import { FileTree } from '@/components/files/FileTree';
import { FileViewer } from '@/components/files/FileViewer';
import { Workstation } from '@/models/workstation';
import { fileBrowser } from '@/models/files';
import { useQuery } from '@/utils/mobx';

const FilesPage = observer(() => {
  const [workstations] = useQuery(Workstation, { autoupdate: true });

  return (
    <Page queries={[workstations]}>
      <div className="flex h-full overflow-hidden">
        <div className="flex w-80 flex-col border-r border-slate-200 bg-white">
          <div className="p-4 pb-1">
            <Select
              value={fileBrowser.workstationId ? String(fileBrowser.workstationId) : ''}
              onChange={(e) =>
                fileBrowser.selectWorkstation(e.target.value ? Number(e.target.value) : null)
              }
            >
              <option value="">Выберите воркстейшн...</option>
              {workstations.items
                .filter((ws) => ws.isReady)
                .map((ws) => (
                  <option key={ws.id} value={String(ws.id)}>
                    {ws.name} ({ws.state})
                  </option>
                ))}
            </Select>
          </div>
          <div className="flex-1 overflow-auto px-4 py-3 text-sm">
            <FileTree />
          </div>
        </div>
        <FileViewer />
      </div>
    </Page>
  );
});

export default FilesPage;
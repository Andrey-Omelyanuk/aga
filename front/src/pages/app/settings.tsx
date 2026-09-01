import { SshKeyPanel } from '@/components/settings/SshKeyPanel';
import { PersonnelPanel } from '@/components/settings/PersonnelPanel';
import { getSshKey } from '@/services/settings';
import { useObject } from '@/utils/mobx';

const SettingsPage = () => {
  const [info, loading] = useObject(getSshKey);

  if (loading) {
    return <div className="p-10 text-slate-400">Загрузка…</div>;
  }

  return (
    <div className="max-w-2xl">
      {info && <SshKeyPanel info={info} />}
      <h2 className="mb-2 mt-6 text-sm font-semibold text-slate-800">Персонал</h2>
      <PersonnelPanel />
    </div>
  );
};

export default SettingsPage;
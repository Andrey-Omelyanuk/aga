import { SshKeyPanel } from '@/components/settings/SshKeyPanel';
import { getSshKey } from '@/services/settings';
import { useObject } from '@/utils/mobx';

const ConfigEnvPage = () => {
  const [info, loading] = useObject(getSshKey);

  if (loading) {
    return <div className="p-10 text-slate-400">Загрузка…</div>;
  }

  return (
    <div className="max-w-2xl">
      {info && <SshKeyPanel info={info} />}
    </div>
  );
};

export default ConfigEnvPage;
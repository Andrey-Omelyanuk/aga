import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { Link } from '@/components/ui/link';
import { Button } from '@/components/ui/button';
import { Tabs, type Tab } from '@/components/ui/tabs';
import { useStore } from '@/store-hooks';
import type { TabName } from '@/models/store';
import { Projects } from '@/pages/Projects';
import { Workstations } from '@/pages/Workstations';
import { Sessions } from '@/pages/Sessions';
import { Personnel } from '@/pages/Personnel';
import { Files } from '@/pages/Files';
import { Chat } from '@/pages/Chat';

const TABS: Array<Tab<TabName>> = [
  { value: 'projects', label: 'Проекты' },
  { value: 'workstations', label: 'Воркстейшны' },
  { value: 'sessions', label: 'Сессии' },
  { value: 'personnel', label: 'Персонал' },
  { value: 'files', label: 'Файлы' },
  { value: 'chat', label: 'Чат' },
];

export const App = observer(function App() {
  const store = useStore();
  const [ready, setReady] = useState(false);
  useEffect(() => {
    void store.init().then(() => setReady(true));
  }, [store]);

  if (!ready) {
    return <div className="h-screen p-10 text-slate-400">Загрузка…</div>;
  }

  return (
    <div className="flex h-screen flex-col">
      <header className="flex items-center gap-4 border-b border-slate-200 bg-white px-5 py-2.5">
        <span className="font-semibold text-slate-800">aga</span>
        <Tabs tabs={TABS} value={store.activeTab} onChange={(v) => store.setActiveTab(v)} />
        <span className="flex-1" />
        {store.showLogin && (
          <Link href={store.loginUrl}>
            <Button variant="outline">Войти через SSO</Button>
          </Link>
        )}
      </header>
      <main className="flex-1 overflow-auto p-5">
        {store.activeTab === 'projects' && <Projects />}
        {store.activeTab === 'workstations' && <Workstations />}
        {store.activeTab === 'sessions' && <Sessions />}
        {store.activeTab === 'personnel' && <Personnel />}
        {store.activeTab === 'files' && <Files />}
        {store.activeTab === 'chat' && <Chat />}
      </main>
    </div>
  );
});
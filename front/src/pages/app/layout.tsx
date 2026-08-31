import { observer } from 'mobx-react-lite';
import { Suspense, useEffect, useState } from 'react';
import { Outlet } from 'react-router-dom';
import { autoResetId, NUMBER, ObjectInput } from 'mobx-model-ui';
import { AppHeader } from '@/components/core/AppHeader';
import { Toaster } from '@/components/core/Toaster';
import { AppContext } from '@/store-hooks';
import { Project } from '@/models/project';
import { useObjectInput, useQuery } from '@/utils/mobx';
import useMobX_ORM from '@/utils/useMobX_ORM';
import me from '@/services/me';

const AppLayout = observer(() => {
  useMobX_ORM();

  const [ready, setReady] = useState(false);
  useEffect(() => {
    void me.init().then(() => setReady(true));
  }, []);

  const [projects] = useQuery(Project, { autoupdate: true });
  // Активный проект — ObjectInput с URL-sync (?project=): глобальный фильтр,
  // на который реактивно смотрят страницы воркстейшнов и сессий.
  const activeProject = useObjectInput(
    () =>
      new ObjectInput(NUMBER(), {
        syncURL: 'project',
        options: projects,
        autoReset: autoResetId,
      }),
    true,
  );

  if (!ready) {
    return <div className="h-screen p-10 text-slate-400">Загрузка…</div>;
  }

  return (
    <AppContext.Provider value={{ activeProject }}>
      <div className="flex h-screen flex-col">
        <AppHeader />
        <main className="flex-1 overflow-auto p-5">
          <Suspense fallback={<div>Загрузка…</div>}>
            <Outlet />
          </Suspense>
        </main>
        <Toaster />
      </div>
    </AppContext.Provider>
  );
});

export default AppLayout;
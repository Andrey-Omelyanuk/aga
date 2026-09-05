import { observer } from 'mobx-react-lite';
import { Suspense, useEffect, useState } from 'react';
import { Outlet } from 'react-router-dom';
import { autoResetId, NUMBER, ObjectInput } from 'mobx-model-ui';
import { AppHeader } from '@/components/core/AppHeader';
import { Toaster } from '@/components/core/Toaster';
import { AppContext } from '@/store-hooks';
import { Project } from '@/models/project';
import { useObjectInput, useQueryCacheSync } from '@/utils/mobx';
import useMobX_ORM from '@/utils/useMobX_ORM';
import me from '@/services/me';
import pub_sub from '@/services/pub-sub';
import LoginPage from './login';

const AppLayout = observer(() => {
  useMobX_ORM();

  const [ready, setReady] = useState(false);
  useEffect(() => {
    void me.init().then(() => setReady(true));
    // Реальное время: подключаемся к центрифуго после входа. Неудача
    // деградирует молча (чат без автообновления, см. историю).
    void pub_sub.init();
  }, []);

  const [projects] = useQueryCacheSync(Project, { autoupdate: true });
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

  // SSO включён и токена нет — UI только для участников, приложение не показываем.
  if (me.show_login && !me.anonymous) {
    return <LoginPage />;
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
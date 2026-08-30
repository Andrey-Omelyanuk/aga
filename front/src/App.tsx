import { observer } from 'mobx-react-lite';
import { useEffect, useState } from 'react';
import { NavLink, Outlet, useLocation, useSearchParams } from 'react-router-dom';
import { Link } from '@/components/ui/link';
import { Button } from '@/components/ui/button';
import { Select } from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { useStore } from '@/store-hooks';
import type { TabName } from '@/models/store';

const TABS: Array<{ tab: TabName; label: string; to: string }> = [
  { tab: 'projects', label: 'Проекты', to: '/projects' },
  { tab: 'workstations', label: 'Воркстейшны', to: '/workstations' },
  { tab: 'sessions', label: 'Сессии', to: '/sessions' },
  { tab: 'personnel', label: 'Персонал', to: '/personnel' },
  { tab: 'files', label: 'Файлы', to: '/files' },
  { tab: 'chat', label: 'Чат', to: '/chat' },
];

function tabFromPath(path: string): TabName {
  const tab = path.split('/')[1] as TabName | undefined;
  return TABS.some((t) => t.tab === tab) ? (tab as TabName) : 'projects';
}

export const App = observer(function App() {
  const store = useStore();
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  const [ready, setReady] = useState(false);
  useEffect(() => {
    void store.init().then(() => setReady(true));
  }, [store]);

  const tab = tabFromPath(location.pathname);
  useEffect(() => {
    void store.ensureLoaded(tab);
  }, [tab]);

  const projectParam = searchParams.get('project');

  // URL → стор: параметр ?project= авторитетен при загрузке (deep-link).
  useEffect(() => {
    if (projectParam === null) return;
    const id = Number(projectParam);
    if (Number.isInteger(id) && id > 0) {
      if (id !== store.activeProjectId) store.setActiveProject(id);
    } else {
      setSearchParams({}, { replace: true });
    }
  }, [projectParam]);

  // Стор → URL: активный проект всегда отражается в URL как глобальный фильтр.
  useEffect(() => {
    const current = store.activeProjectId;
    if (current === null) {
      if (projectParam !== null) setSearchParams({}, { replace: true });
    } else if (projectParam !== String(current)) {
      setSearchParams({ project: String(current) }, { replace: true });
    }
  }, [store.activeProjectId]);

  const onProjectChange = (id: number | null) => {
    store.setActiveProject(id);
  };

  if (!ready) {
    return <div className="h-screen p-10 text-slate-400">Загрузка…</div>;
  }

  return (
    <div className="flex h-screen flex-col">
      <header className="flex items-center gap-4 border-b border-slate-200 bg-white px-5 py-2.5">
        <span className="font-semibold text-slate-800">aga</span>
        <label className="flex items-center gap-2 text-sm text-slate-500">
          Проект
          <Select
            className="h-8"
            value={store.activeProjectId === null ? '' : String(store.activeProjectId)}
            onChange={(e) =>
              onProjectChange(e.target.value ? Number(e.target.value) : null)
            }
          >
            {store.projects.length === 0 ? (
              <option value="">Проектов нет</option>
            ) : (
              <>
                <option value="">— все проекты —</option>
                {store.projects.map((p) => (
                  <option key={p.id} value={String(p.id)}>
                    {p.git_url}
                  </option>
                ))}
              </>
            )}
          </Select>
        </label>
        <nav className="flex gap-1">
          {TABS.map(({ tab: t, label, to }) => (
            <NavLink
              key={t}
              to={{ pathname: to, search: location.search }}
              className={({ isActive }) =>
                cn(
                  'rounded-md px-4 py-2 text-sm cursor-pointer',
                  isActive
                    ? 'bg-blue-100 font-semibold text-blue-700'
                    : 'text-slate-500 hover:bg-slate-100',
                )
              }
            >
              {label}
            </NavLink>
          ))}
        </nav>
        <span className="flex-1" />
        {store.showLogin && (
          <Link href={store.loginUrl}>
            <Button variant="outline">Войти через SSO</Button>
          </Link>
        )}
      </header>
      <main className="flex-1 overflow-auto p-5">
        <Outlet />
      </main>
    </div>
  );
});
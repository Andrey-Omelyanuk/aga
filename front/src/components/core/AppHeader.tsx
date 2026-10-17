import { observer } from 'mobx-react-lite';
import { NavLink, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import me from '@/services/me';
import { useApp } from '@/store-hooks';
import { SelectInput } from '@/components/core/inputs';

export type TabName =
  | 'projects'
  | 'workstations'
  | 'sessions'
  | 'files'
  | 'chat'
  | 'config';

export const AppHeader = observer(() => {
  const { t } = useTranslation();
  const { activeProject } = useApp();
  const location = useLocation();
  const configOpen = location.pathname.startsWith('/config');

  const TABS: Array<{ tab: TabName; label: string; to: string }> = [
    { tab: 'projects', label: t('navigation.projects'), to: '/projects' },
    { tab: 'workstations', label: t('navigation.workstations'), to: '/workstations' },
    { tab: 'sessions', label: t('navigation.sessions'), to: '/sessions' },
    { tab: 'files', label: t('navigation.files'), to: '/files' },
    { tab: 'chat', label: t('navigation.chat'), to: '/chat' },
    { tab: 'config', label: t('navigation.config'), to: '/config/env' },
  ];

  // Второе топ-меню: появляется, когда открыт Config.
  const CONFIG_MENU: Array<{ tab: string; label: string; to: string }> = [
    { tab: 'env', label: t('navigation.env'), to: '/config/env' },
    { tab: 'users', label: t('navigation.users'), to: '/config/users' },
    { tab: 'skills', label: t('navigation.skills'), to: '/config/skills' },
    { tab: 'shortcuts', label: t('navigation.shortcuts'), to: '/config/shortcuts' },
    { tab: 'agent-sets', label: t('navigation.agentSets'), to: '/config/agent-sets' },
    { tab: 'llms', label: t('navigation.llms'), to: '/config/llms' },
    { tab: 'mcp', label: t('navigation.mcp'), to: '/config/mcp' },
    { tab: 'help', label: t('navigation.help'), to: '/config/help' },
  ];

  const navItemClass = ({ isActive }: { isActive: boolean }) =>
    cn(
      'rounded-md px-4 py-2 text-sm cursor-pointer',
      isActive
        ? 'bg-blue-100 font-semibold text-blue-700'
        : 'text-slate-500 hover:bg-slate-100',
    );

  return (
    <header className="border-b border-slate-200 bg-white">
      <div className="flex items-center gap-4 px-5 py-2.5">
        <span className="font-semibold text-slate-800">aga</span>
        <SelectInput
          input={activeProject}
          optionKey={(p) => String(p.id)}
          optionLabel={(p) => p.git_url}
          emptyLabel={t('navigation.allProjects')}
          className="h-8"
        />
        <nav className="flex gap-1">
          {TABS.map(({ tab: t2, label, to }) => (
            <NavLink
              key={t2}
              to={{ pathname: to, search: location.search }}
              className={({ isActive }) =>
                cn(
                  'rounded-md px-4 py-2 text-sm cursor-pointer',
                  (t2 === 'config' ? configOpen : isActive)
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
        {me.user && !me.anonymous && (
          <NavLink
            to={{ pathname: '/profile', search: location.search }}
            className="flex items-center gap-2 text-sm font-medium text-slate-700 hover:text-blue-700"
            title={t('navigation.profile')}
          >
            <span>{me.user.name}</span>
            {me.user.is_super_user && <Badge variant="warn">admin</Badge>}
          </NavLink>
        )}
      </div>
      {configOpen && (
        <nav className="flex gap-1 border-t border-slate-100 bg-slate-50 px-5 py-1.5">
          {CONFIG_MENU.map(({ tab: t2, label, to }) => (
            <NavLink
              key={t2}
              to={{ pathname: to, search: location.search }}
              className={navItemClass}
            >
              {label}
            </NavLink>
          ))}
        </nav>
      )}
    </header>
  );
});
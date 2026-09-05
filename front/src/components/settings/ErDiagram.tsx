import type { HelpLang } from './AgentSetDiagram';

interface ErLabels {
  title: string;
  agentSet: string;
  agentSetAttrs: string[];
  contains: string;
  agent: string;
  agentAttrs: string[];
  skill: string;
  skillAttrs: string[];
  skillsBy: string;
  command: string;
  commandAttrs: string[];
  commandsBy: string;
  llm: string;
  llmAttrs: string[];
  llmChoice: string;
  users: string;
  usersNote: string;
  env: string;
  envNote: string;
  standalone: string;
}

const LABELS: Record<HelpLang, ErLabels> = {
  ru: {
    title: 'ERP: связи сущностей конфига',
    agentSet: 'Agent Set',
    agentSetAttrs: ['название', 'прикреплён к проекту'],
    contains: 'содержит агентов',
    agent: 'Agent',
    agentAttrs: ['папка (имя агента)', 'правила', 'территория', 'инструменты'],
    skill: 'Skill',
    skillAttrs: ['название', 'содержимое'],
    skillsBy: 'выдаётся по имени из каталога',
    command: 'Command',
    commandAttrs: ['название', 'содержимое'],
    commandsBy: 'выдаётся по имени из каталога',
    llm: 'LLM',
    llmAttrs: ['название', 'url, ключ, модель', 'одно — дефолтная'],
    llmChoice: 'выбор подключения; без выбора — дефолтная',
    users: 'Users',
    usersNote: 'участники из SSO (Keycloak)',
    env: 'Env',
    envNote: 'SSH-ключ aga: git-доступ воркстейшнов',
    standalone: 'отдельные разделы',
  },
  en: {
    title: 'ERP: config entity relationships',
    agentSet: 'Agent Set',
    agentSetAttrs: ['name', 'attached to a project'],
    contains: 'contains agents',
    agent: 'Agent',
    agentAttrs: ['folder (agent name)', 'rules', 'territory', 'tools'],
    skill: 'Skill',
    skillAttrs: ['name', 'content'],
    skillsBy: 'given by name from the catalog',
    command: 'Command',
    commandAttrs: ['name', 'content'],
    commandsBy: 'given by name from the catalog',
    llm: 'LLM',
    llmAttrs: ['name', 'url, key, model', 'one is default'],
    llmChoice: 'picks a connection; without one — default LLM',
    users: 'Users',
    usersNote: 'members from SSO (Keycloak)',
    env: 'Env',
    envNote: 'aga SSH key: workstation git access',
    standalone: 'separate sections',
  },
};

function EntityBox({ name, attrs }: { name: string; attrs: string[] }) {
  return (
    <div className="w-44 rounded-lg border-2 border-slate-400 bg-white p-2.5 text-xs shadow-sm">
      <div className="mb-1 border-b border-slate-200 pb-1 text-center font-semibold text-slate-800">
        {name}
      </div>
      <ul className="space-y-0.5 text-slate-600">
        {attrs.map((a) => (
          <li key={a}>{a}</li>
        ))}
      </ul>
    </div>
  );
}

function RelationColumn({ label, box }: { label: string; box: React.ReactNode }) {
  return (
    <div className="flex w-44 flex-col items-center">
      <div className="h-4 w-px bg-slate-300" />
      <span className="text-center text-[10px] text-slate-500">{label}</span>
      <div className="h-4 w-px bg-slate-300" />
      {box}
    </div>
  );
}

// ERP-диаграмма связей сущностей конфига: HTML-разметка, как и диаграмма
// состава набора (без библиотек диаграмм). Статическая схема, не данные.
export const ErDiagram = ({ lang }: { lang: HelpLang }) => {
  const t = LABELS[lang];
  return (
    <div
      data-testid="er-diagram"
      className="mt-4 overflow-x-auto rounded-xl border border-slate-200 bg-slate-50 p-4"
    >
      <div className="mb-5 text-center text-xs font-medium text-slate-500">{t.title}</div>
      <div className="flex flex-col items-center">
        <EntityBox name={t.agentSet} attrs={t.agentSetAttrs} />
        <div className="flex flex-col items-center">
          <div className="h-3 w-px bg-slate-300" />
          <span className="text-[10px] text-slate-500">{t.contains}</span>
          <div className="h-3 w-px bg-slate-300" />
        </div>
        <EntityBox name={t.agent} attrs={t.agentAttrs} />
        <div className="mt-3 w-full">
          <div className="flex justify-around border-t border-slate-300 pt-4">
            <RelationColumn
              label={t.skillsBy}
              box={<EntityBox name={t.skill} attrs={t.skillAttrs} />}
            />
            <RelationColumn
              label={t.commandsBy}
              box={<EntityBox name={t.command} attrs={t.commandAttrs} />}
            />
            <RelationColumn
              label={t.llmChoice}
              box={<EntityBox name={t.llm} attrs={t.llmAttrs} />}
            />
          </div>
        </div>
        <div className="mt-5 w-full border-t border-dashed border-slate-200 pt-3">
          <div className="mb-2 text-center text-[10px] text-slate-400">{t.standalone}</div>
          <div className="flex justify-center gap-6">
            <EntityBox name={t.users} attrs={[t.usersNote]} />
            <EntityBox name={t.env} attrs={[t.envNote]} />
          </div>
        </div>
      </div>
    </div>
  );
};
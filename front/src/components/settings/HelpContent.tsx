import { AgentSetDiagram, type HelpLang } from './AgentSetDiagram';
import { ErDiagram } from './ErDiagram';

// Статическая справка: текст одинаков при любой настройке платформы, к данным
// не ходит. Поддерживать актуальность — вручную, при изменении состава конфига.
const CONTENT: Record<
  HelpLang,
  {
    agentSetTitle: string;
    agentSetIntro: string;
    configTitle: string;
    configIntro: string;
    sections: Array<{ name: string; text: string }>;
  }
> = {
  ru: {
    agentSetTitle: 'Что такое набор агентов (Agent Set)',
    agentSetIntro:
      'Набор агентов — команда агентов для проекта: агенты образуют дерево по ' +
      'иерархии папок проекта, у каждой папки — свой агент, у подпапок — его ' +
      'наследники. У каждого агента своя территория, правила, данные скиллы и ' +
      'команды (по имени из каталога), инструменты и выбранное подключение к LLM.',
    configTitle: 'Из чего состоит конфиг',
    configIntro: 'Раздел Config веб-клиента состоит из шести частей:',
    sections: [
      {
        name: 'Env',
        text: 'публичный SSH-ключ aga: доступ воркстейшнов к git-репозиториям проекта; ключ задаёт админ в окружении ядра',
      },
      {
        name: 'Users',
        text: 'персонал: участники приходят из SSO (Keycloak), внутри aga учётки не редактируются',
      },
      {
        name: 'Skills',
        text: 'каталог скиллов: у записи одно текущее содержимое, правки пишутся в историю',
      },
      {
        name: 'Commands',
        text: 'каталог команд: то же, что скиллы, для исполняемых команд',
      },
      {
        name: 'Agent Set',
        text: 'наборы агентов: создание набора и прикрепление его к проекту',
      },
      {
        name: 'LLM',
        text: 'подключения к LLM: название, url, ключ и модель; у агента набора выбирается подключение, одно из подключений — дефолтная LLM',
      },
    ],
  },
  en: {
    agentSetTitle: 'What is an Agent Set',
    agentSetIntro:
      'An Agent Set is a team of agents for a project: agents form a tree that ' +
      'follows the project folder hierarchy, each folder has its own agent and ' +
      'subfolders are its successors. Every agent has its own territory, rules, ' +
      'given skills and commands (by name from the catalog), tools and a chosen ' +
      'LLM connection.',
    configTitle: 'What the config consists of',
    configIntro: 'The Config section of the web client has six parts:',
    sections: [
      {
        name: 'Env',
        text: 'the public aga SSH key: workstation access to project git repositories; set by the admin in the core environment',
      },
      {
        name: 'Users',
        text: 'the personnel: members come from SSO (Keycloak), accounts are not edited inside aga',
      },
      {
        name: 'Skills',
        text: 'the skills catalog: a record has one current content, edits are written to history',
      },
      {
        name: 'Commands',
        text: 'the commands catalog: same as skills, for executable commands',
      },
      {
        name: 'Agent Set',
        text: 'agent sets: creating a set and attaching it to a project',
      },
      {
        name: 'LLM',
        text: 'LLM connections: name, url, key and model; each agent of a set picks a connection, one connection is the default LLM',
      },
    ],
  },
};

export const HelpContent = ({ lang }: { lang: HelpLang }) => {
  const t = CONTENT[lang];
  return (
    <div className="space-y-8">
      <section>
        <h2 className="text-base font-semibold text-slate-800">{t.agentSetTitle}</h2>
        <p className="mt-1 text-sm text-slate-600">{t.agentSetIntro}</p>
        <AgentSetDiagram lang={lang} />
      </section>
      <section>
        <h2 className="text-base font-semibold text-slate-800">{t.configTitle}</h2>
        <p className="mt-1 text-sm text-slate-600">{t.configIntro}</p>
        <ul className="mt-3 space-y-2">
          {t.sections.map((s) => (
            <li key={s.name} className="text-sm text-slate-600">
              <span className="font-medium text-slate-800">{s.name}</span> — {s.text}
            </li>
          ))}
        </ul>
        <ErDiagram lang={lang} />
      </section>
    </div>
  );
};
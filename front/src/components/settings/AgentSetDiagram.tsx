// Языки справки. Пока два: русский (по умолчанию) и английский.
export type HelpLang = 'ru' | 'en';

interface DiagramLabels {
  agentSet: string;
  territory: string;
  skills: string;
  commands: string;
  tools: string;
  llm: string;
  defaultLlm: string;
}

const LABELS: Record<HelpLang, DiagramLabels> = {
  ru: {
    agentSet: 'Набор агентов',
    territory: 'Территория',
    skills: 'Скиллы',
    commands: 'Команды',
    tools: 'Инструменты',
    llm: 'LLM',
    defaultLlm: 'дефолтная',
  },
  en: {
    agentSet: 'Agent Set',
    territory: 'Territory',
    skills: 'Skills',
    commands: 'Commands',
    tools: 'Tools',
    llm: 'LLM',
    defaultLlm: 'default',
  },
};

interface TreeNode {
  name: string;
  territory: string;
  skills: string[];
  commands: string[];
  tools: string[];
  /** Имя подключения к LLM или 'default' — дефолтная LLM со страницы «LLM». */
  llm: string;
  children?: TreeNode[];
}

// Пример дерева набора: агент корня, у подпапок — наследники. Статическая
// схема состава, а не реальные данные платформы.
const TREE: TreeNode = {
  name: 'src/',
  territory: 'src/',
  skills: ['review'],
  commands: ['deploy'],
  tools: ['git', 'make'],
  llm: 'ollama-local',
  children: [
    {
      name: 'src/backend',
      territory: 'src/backend',
      skills: ['lint'],
      commands: ['build'],
      tools: ['cargo', 'make'],
      llm: 'default',
    },
    {
      name: 'src/frontend',
      territory: 'src/frontend',
      skills: ['design'],
      commands: [],
      tools: ['npm'],
      llm: 'ollama-local',
    },
  ],
};

function AgentBox({ node, labels }: { node: TreeNode; labels: DiagramLabels }) {
  return (
    <div className="w-52 rounded-lg border border-slate-300 bg-white p-2.5 text-xs shadow-sm">
      <div className="mb-1 font-semibold text-slate-800">{node.name}</div>
      <div className="space-y-0.5 text-slate-600">
        <div>
          <span className="text-slate-400">{labels.territory}:</span> {node.territory}
        </div>
        <div>
          <span className="text-slate-400">{labels.skills}:</span>{' '}
          {node.skills.join(', ') || '—'}
        </div>
        <div>
          <span className="text-slate-400">{labels.commands}:</span>{' '}
          {node.commands.join(', ') || '—'}
        </div>
        <div>
          <span className="text-slate-400">{labels.tools}:</span> {node.tools.join(', ')}
        </div>
        <div>
          <span className="text-slate-400">{labels.llm}:</span>{' '}
          {node.llm === 'default' ? labels.defaultLlm : node.llm}
        </div>
      </div>
    </div>
  );
}

function Tree({ node, labels }: { node: TreeNode; labels: DiagramLabels }) {
  const children = node.children ?? [];
  return (
    <div className="flex flex-col items-center">
      <AgentBox node={node} labels={labels} />
      {children.length > 0 && (
        <div className="flex flex-col items-center">
          <div className="h-4 w-px bg-slate-300" />
          <div className="flex items-start gap-8 border-t border-slate-300 pt-4">
            {children.map((c) => (
              <Tree key={c.name} node={c} labels={labels} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

// Диаграмма состава набора агентов: HTML-дерево (без библиотек диаграмм).
export const AgentSetDiagram = ({ lang }: { lang: HelpLang }) => {
  const labels = LABELS[lang];
  return (
    <div
      data-testid="agent-set-diagram"
      className="mt-4 overflow-x-auto rounded-xl border border-slate-200 bg-slate-50 p-4"
    >
      <div className="mb-5 rounded-lg border-2 border-blue-200 bg-blue-50 px-3 py-2 text-center">
        <div className="text-sm font-semibold text-blue-800">Agent Set</div>
        <div className="text-xs text-blue-600">{labels.agentSet}</div>
      </div>
      <Tree node={TREE} labels={labels} />
    </div>
  );
};
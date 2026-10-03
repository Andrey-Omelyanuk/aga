import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { AgentSetEditor } from './AgentSetEditor';
import type { Agent, CatalogItem, Llm, McpServer } from '@/models/project';
import type { User } from '@/models/core';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderEditor(
  agents: Agent[],
  skills: CatalogItem[],
  connections: Llm[] = [],
  users: User[] = [],
  mcpServers: McpServer[] = [],
) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(
      <AgentSetEditor
        setId={1}
        name="ops"
        agents={agents}
        skills={skills}
        connections={connections}
        users={users}
        mcpServers={mcpServers}
        onSaved={() => {}}
      />,
    );
  });
  return { container, root };
}

describe('AgentSetEditor', () => {
  it('shows composition: agents, their territory, given skills by name, tools', () => {
    const agent: Agent = {
      id: 10,
      name: 'src/backend',
      description: 'Правила бэкенда',
      tools: ['git', 'make'],
      max_iterations: 3,
      llm_id: null,
      parent_id: null,
      skills: [{ name: 'review' }],
      territory: { folder: 'src/backend', excludes: ['src/backend/api'] },
    };
    const skills: CatalogItem[] = [
      {
        id: 1,
        name: 'review',
        content: 'Проверять диф и тесты',
        deleted: false,
      },
    ];

    const { container, root } = renderEditor([agent], skills);
    const text = container.textContent ?? '';

    // Агент, его территория (папка + чужие папки).
    expect(text).toContain('src/backend');
    expect(text).toContain('Правила бэкенда');
    expect(text).toContain('src/backend/api');
    // Инструменты — список, каждый элемент виден.
    expect(text).toContain('git');
    expect(text).toContain('make');
    // Данные скиллы — по имени, без версии.
    expect(text).toContain('review');
    // Фиксации версий в составе набора больше нет.
    expect(text).not.toContain('версия');

    act(() => root.unmount());
  });

  it('agent picks a connection to LLM; own model and temperature are gone', () => {
    const agent: Agent = {
      id: 10,
      name: 'src/backend',
      description: 'Правила бэкенда',
      tools: ['git'],
      max_iterations: 3,
      llm_id: 7,
      parent_id: null,
      skills: [],
      territory: { folder: 'src/backend', excludes: [] },
    };
    const connections: Llm[] = [
      {
        id: 7,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        api_key: 'secret',
        model_name: 'qwen3:0.6b',
        is_default: true,
      },
    ] as Llm[];

    const { container, root } = renderEditor([agent], [], connections);
    const text = container.textContent ?? '';
    // В редакторе видно подключение к LLM — выбранное и из списка созданных.
    expect(text).toContain('ollama-local');
    // Своей модели и температуры у агента в редакторе нет.
    expect(text).not.toContain('temperature');
    expect(text).not.toContain('модел');

    act(() => root.unmount());
  });

  it('shows the chat user the agent listens to, by name, selected', () => {
    const agent: Agent = {
      id: 10,
      name: 'src/backend',
      description: 'Правила бэкенда',
      tools: ['git'],
      max_iterations: 3,
      llm_id: null,
      parent_id: null,
      skills: [],
      territory: { folder: 'src/backend', excludes: [] },
      listen_user_id: 7,
    };
    const users = [
      { id: 7, name: 'alice', kind: 'human' },
      { id: 8, name: 'Agent.Bot', kind: 'agent' },
    ] as unknown as User[];

    const { container, root } = renderEditor([agent], [], [], users);
    // Пользователя видно в редакторе: он выбран в селекторе прослушивания.
    const selects = Array.from(container.querySelectorAll('select'));
    const listener = selects.find((s) =>
      Array.from((s as HTMLSelectElement).options).some((o) => o.value === '7'),
    ) as HTMLSelectElement | undefined;
    expect(listener, 'есть селектор слушаемого пользователя').toBeTruthy();
    expect(listener!.value).toBe('7');
    expect(listener!.selectedOptions[0].textContent).toContain('alice');
    // Агент-пользователи в список прослушивания не попадают.
    const options = Array.from(listener!.options).map((o) => o.textContent);
    expect(options.some((t) => t?.includes('Agent.Bot'))).toBe(false);
    // Люди — видны.
    expect(options.some((t) => t?.includes('alice'))).toBe(true);

    act(() => root.unmount());
  });

  it('each agent gets its own skills and MCP servers', () => {
    const base = {
      description: '',
      tools: [],
      max_iterations: 3,
      llm_id: null,
      parent_id: null,
    };
    const agents: Agent[] = [
      { ...base, id: 1, name: 'a', skills: [{ name: 'review' }], mcp: ['github'],
        territory: { folder: 'a', excludes: [] } },
      { ...base, id: 2, name: 'b', skills: [], mcp: [],
        territory: { folder: 'b', excludes: [] } },
    ];
    const skills: CatalogItem[] = [{ id: 1, name: 'review', content: '', deleted: false }];
    const servers = [
      { id: 1, name: 'github', transport: 'http', url: 'http://x', command: '', api_key: null },
      { id: 2, name: 'fs', transport: 'stdio', url: '', command: 'fs', api_key: null },
    ] as McpServer[];
    const { container, root } = renderEditor(agents, skills, [], [], servers);
    // Чекбоксы скиллов и MCP — по одному набору на агента, со своими отметками.
    const boxes = (label: string) =>
      Array.from(container.querySelectorAll<HTMLInputElement>(`input[aria-label="${label}"]`));
    expect(boxes('MCP github').map((b) => b.checked)).toEqual([true, false]);
    expect(boxes('MCP fs').map((b) => b.checked)).toEqual([false, false]);
    const skillBoxes = Array.from(container.querySelectorAll('label'))
      .filter((l) => l.textContent === 'review')
      .map((l) => (l.querySelector('input') as HTMLInputElement).checked);
    expect(skillBoxes).toEqual([true, false]);
    act(() => boxes('MCP fs')[1].click());
    expect(boxes('MCP fs').map((b) => b.checked)).toEqual([false, true]);
    act(() => root.unmount());
  });
});

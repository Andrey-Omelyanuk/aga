import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { AgentSetEditor } from './AgentSetEditor';
import type { Agent, CatalogItem, Llm } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderEditor(
  agents: Agent[],
  skills: CatalogItem[],
  commands: CatalogItem[],
  connections: Llm[] = [],
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
        commands={commands}
        connections={connections}
        onSaved={() => {}}
      />,
    );
  });
  return { container, root };
}

describe('AgentSetEditor', () => {
  it('shows composition: agents, their territory, given skills/commands by name, tools', () => {
    const agent: Agent = {
      id: 10,
      name: 'src/backend',
      description: 'Правила бэкенда',
      tools: ['git', 'make'],
      max_iterations: 3,
      llm_id: null,
      parent_id: null,
      skills: [{ name: 'review' }],
      commands: [{ name: 'deploy' }],
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
    const commands: CatalogItem[] = [
      { id: 1, name: 'deploy', content: 'Выкат', deleted: false },
    ];

    const { container, root } = renderEditor([agent], skills, commands);
    const text = container.textContent ?? '';

    // Агент, его территория (папка + чужие папки).
    expect(text).toContain('src/backend');
    expect(text).toContain('Правила бэкенда');
    expect(text).toContain('src/backend/api');
    // Инструменты — список, каждый элемент виден.
    expect(text).toContain('git');
    expect(text).toContain('make');
    // Данные скиллы и команды — по имени, без версии.
    expect(text).toContain('review');
    expect(text).toContain('deploy');
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
      commands: [],
      territory: { folder: 'src/backend', excludes: [] },
    };
    const connections: Llm[] = [
      {
        id: 7,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        api_key: 'secret',
      },
    ] as Llm[];

    const { container, root } = renderEditor([agent], [], [], connections);
    const text = container.textContent ?? '';
    // В редакторе видно подключение к LLM — выбранное и из списка созданных.
    expect(text).toContain('ollama-local');
    // Своей модели и температуры у агента в редакторе нет.
    expect(text).not.toContain('temperature');
    expect(text).not.toContain('модел');

    act(() => root.unmount());
  });
});
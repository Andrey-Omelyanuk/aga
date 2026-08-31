import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { AgentSetEditor } from './AgentSetEditor';
import type { Agent, CatalogItem } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderEditor(agents: Agent[], skills: CatalogItem[], commands: CatalogItem[]) {
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
        onSaved={() => {}}
      />,
    );
  });
  return { container, root };
}

describe('AgentSetEditor', () => {
  it('shows composition: agents, their territory, given skills/commands with versions, tools', () => {
    const agent: Agent = {
      id: 10,
      name: 'src/backend',
      description: 'Правила бэкенда',
      tools: ['git', 'make'],
      max_iterations: 3,
      temperature: 0.7,
      parent_id: null,
      skills: [{ name: 'review', pinned_version: '1' }],
      commands: [{ name: 'deploy', pinned_version: null }],
      territory: { folder: 'src/backend', excludes: ['src/backend/api'] },
    };
    const skills: CatalogItem[] = [
      {
        id: 1,
        name: 'review',
        versions: [
          { version: '1', content: 'Диф' },
          { version: '2', content: 'Диф и тесты' },
        ],
      },
    ];
    const commands: CatalogItem[] = [
      { id: 1, name: 'deploy', versions: [{ version: '1', content: 'Выкат' }] },
    ];

    const { container, root } = renderEditor([agent], skills, commands);
    const text = container.textContent ?? '';

    // Агент, его территория (папка + чужие папки).
    expect(text).toContain('src/backend');
    expect(text).toContain('Правила бэкенда');
    expect(text).toContain('src/backend/api');
    // Инструменты — значение поля ввода (в textContent инпута нет).
    const toolsInput = container.querySelector<HTMLInputElement>('input[placeholder*="git, make"]');
    expect(toolsInput?.value).toContain('git');
    expect(toolsInput?.value).toContain('make');
    // Данные скиллы и команды с версиями.
    expect(text).toContain('review');
    expect(text).toContain('версия 1');
    expect(text).toContain('deploy');

    act(() => root.unmount());
  });
});
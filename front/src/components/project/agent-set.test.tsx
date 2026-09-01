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
  it('shows composition: agents, their territory, given skills/commands by name, tools', () => {
    const agent: Agent = {
      id: 10,
      name: 'src/backend',
      description: 'Правила бэкенда',
      tools: ['git', 'make'],
      max_iterations: 3,
      temperature: 0.7,
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
});
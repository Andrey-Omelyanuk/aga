import { describe, expect, it, beforeEach } from 'vitest';
import { act } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';
import { NUMBER, ObjectInput, type Query } from 'mobx-model-ui';
import { AppHeader } from '@/components/core/AppHeader';
import { AppContext } from '@/store-hooks';
import type { Project } from '@/models/project';
import ConfigHelpPage, { HELP_LANG_KEY } from '@/pages/app/configHelp';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function renderHelp() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => {
    root.render(
      <MemoryRouter initialEntries={['/config/help']}>
        <Routes>
          <Route path="config/help" element={<ConfigHelpPage />} />
        </Routes>
      </MemoryRouter>,
    );
  });
  return { container, root };
}

function stubProjectInput(): ObjectInput<Project> {
  return new ObjectInput(NUMBER(), {
    options: [] as unknown as Query<Project>,
  }) as ObjectInput<Project>;
}

beforeEach(() => {
  localStorage.clear();
});

describe('ConfigHelpPage', () => {
  it('Config tab menu has a Help item opening the help page', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root: Root = createRoot(container);
    await act(async () => {
      root.render(
        <AppContext.Provider value={{ activeProject: stubProjectInput() }}>
          <MemoryRouter initialEntries={['/config/env']}>
            <AppHeader />
          </MemoryRouter>
        </AppContext.Provider>,
      );
    });

    const link = container.querySelector('a[href="/config/help"]');
    expect(link).not.toBeNull();
    expect(link?.textContent).toBe('Help');

    act(() => root.unmount());
  });

  it('route /config/help opens the help page', async () => {
    const { container, root } = await renderHelp();
    const text = container.textContent ?? '';
    expect(text).toContain('Help');
    expect(text).toContain('Что такое набор агентов');
    act(() => root.unmount());
  });

  it('shows the agent set composition as a diagram: agents by folder tree, territory, skills/commands by name, tools, LLM connection', async () => {
    const { container, root } = await renderHelp();
    const diagram = container.querySelector('[data-testid="agent-set-diagram"]');
    expect(diagram).not.toBeNull();
    const text = diagram?.textContent ?? '';

    // Агенты деревом по папкам проекта: корень и наследники.
    expect(text).toContain('src/backend');
    expect(text).toContain('src/frontend');
    // У каждого агента территория.
    expect(text).toContain('Территория');
    // Данные скиллы и команды — по имени.
    expect(text).toContain('review');
    expect(text).toContain('deploy');
    expect(text).toContain('lint');
    expect(text).toContain('build');
    // Инструменты.
    expect(text).toContain('git');
    expect(text).toContain('make');
    expect(text).toContain('npm');
    // Выбранное подключение к LLM и дефолтная LLM.
    expect(text).toContain('ollama-local');
    expect(text).toContain('дефолтная');

    act(() => root.unmount());
  });

  it('describes the whole config: Env, Users, Skills, Commands, Agent Set, LLM', async () => {
    const { container, root } = await renderHelp();
    const text = container.textContent ?? '';
    for (const name of ['Env', 'Users', 'Skills', 'Commands', 'Agent Set', 'LLM']) {
      expect(text).toContain(name);
    }
    expect(text).toContain('Из чего состоит конфиг');
    act(() => root.unmount());
  });

  it('shows an ERP diagram of config entity relationships', async () => {
    const { container, root } = await renderHelp();
    const diagram = container.querySelector('[data-testid="er-diagram"]');
    expect(diagram).not.toBeNull();
    const text = diagram?.textContent ?? '';

    // Набор содержит агентов; агент получает скиллы и команды по имени.
    expect(text).toContain('содержит агентов');
    expect(text).toContain('выдаётся по имени из каталога');
    // Агент выбирает подключение к LLM; без выбора — дефолтная.
    expect(text).toContain('выбор подключения; без выбора — дефолтная');
    expect(text).toContain('дефолтная');
    // Env и Users — отдельные разделы.
    expect(text).toContain('участники из SSO');
    expect(text).toContain('SSH-ключ');
    expect(text).toContain('отдельные разделы');

    act(() => root.unmount());
  });

  it('shows Russian on the first open', async () => {
    const { container, root } = await renderHelp();
    const text = container.textContent ?? '';
    expect(text).toContain('Что такое набор агентов');
    expect(text).toContain('Из чего состоит конфиг');
    act(() => root.unmount());
  });

  it('switch to English changes the language and the choice persists across remounts', async () => {
    const first = await renderHelp();
    const enBtn = Array.from(first.container.querySelectorAll('button')).find(
      (b) => b.textContent === 'EN',
    );
    expect(enBtn).toBeTruthy();

    act(() => enBtn!.click());

    expect(first.container.textContent).toContain('What is an Agent Set');
    expect(first.container.textContent).not.toContain('Что такое набор агентов');
    expect(localStorage.getItem(HELP_LANG_KEY)).toBe('en');

    act(() => first.root.unmount());

    const second = await renderHelp();
    expect(second.container.textContent).toContain('What is an Agent Set');
    act(() => second.root.unmount());
  });

  it('renders the same documentation regardless of configuration: a static page, no data', async () => {
    const { container, root } = await renderHelp();
    const text = container.textContent ?? '';
    // Страница не ждёт никаких данных: контент есть сразу, спиннера нет.
    expect(text).not.toContain('Загрузка…');
    expect(text).toContain('Что такое набор агентов');
    expect(text).toContain('Из чего состоит конфиг');
    act(() => root.unmount());
  });
});
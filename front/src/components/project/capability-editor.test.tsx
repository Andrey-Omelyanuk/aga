import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';
import { CapabilityEditor } from './CapabilityEditor';
import type { CatalogItem } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderEditor(
  items: CatalogItem[],
  deleted: CatalogItem[],
  kind: 'skills' | 'shortcuts' = 'skills',
) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(
      <MemoryRouter>
        <CapabilityEditor kind={kind} items={items} deleted={deleted} onChanged={() => {}} />
      </MemoryRouter>,
    );
  });
  return { container, root };
}

function clickItem(container: HTMLElement, name: string) {
  const row = [...container.querySelectorAll('button')].find(
    (b) => b.textContent?.trim() === name,
  );
  expect(row, `row for ${name}`).toBeDefined();
  act(() => row!.dispatchEvent(new MouseEvent('click', { bubbles: true })));
}

describe('CapabilityEditor', () => {
  it('lists skills on the left and edits the selected one on the right', () => {
    const items: CatalogItem[] = [
      { id: 1, name: 'review', content: 'Проверять диф и тесты', deleted: false },
    ];
    const { container, root } = renderEditor(items, []);
    const text = container.textContent ?? '';

    // Список слева содержит имя записи.
    expect(text).toContain('review');
    // Редактор появляется после выбора записи.
    clickItem(container, 'review');
    expect(container.querySelector('input[value="review"]')).not.toBeNull();
    expect(container.textContent).toContain('Проверять диф и тесты');
    // История открывается на конкретную запись.
    expect(container.querySelector('a[href="/skills/1/history"]')).not.toBeNull();

    act(() => root.unmount());
  });

  it('shows deleted records in the «Удалённые» list with their history', () => {
    const deleted: CatalogItem[] = [
      { id: 3, name: 'old-skill', content: '', deleted: true },
    ];
    const { container, root } = renderEditor([], deleted);
    const text = container.textContent ?? '';

    expect(text).toContain('Удалённые');
    expect(text).toContain('old-skill');
    expect(container.querySelector('a[href="/skills/3/history"]')).not.toBeNull();

    act(() => root.unmount());
  });

  it('edits shortcuts: name and text on the right, history on the record', () => {
    const items: CatalogItem[] = [
      { id: 7, name: 'review', content: 'Проверять диф', deleted: false },
    ];
    const { container, root } = renderEditor(items, [], 'shortcuts');
    const text = container.textContent ?? '';

    // Сокращения — отдельный вид: заголовок создания и ссылка на историю
    // ведут на /shortcuts, а не на скиллы.
    expect(text).toContain('Новое сокращение');
    expect(text).toContain('review');
    expect(container.querySelector('a[href="/shortcuts/7/history"]')).not.toBeNull();
    clickItem(container, 'review');
    expect(container.textContent).toContain('Проверять диф');

    act(() => root.unmount());
  });
});
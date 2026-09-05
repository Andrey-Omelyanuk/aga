import { describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';

vi.mock('@/services/http', () => ({
  default: { get: vi.fn() },
}));

import http from '@/services/http';
import ChangesPage from '@/pages/app/changes';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function renderChanges(id: string) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => {
    root.render(
      <MemoryRouter initialEntries={[`/workstations/${id}/changes`]}>
        <Routes>
          <Route path="workstations/:id/changes" element={<ChangesPage />} />
        </Routes>
      </MemoryRouter>,
    );
  });
  await act(async () => {});
  return { container, root };
}

describe('ChangesPage', () => {
  it('при отсутствии изменений страница показывает «Изменений нет»', async () => {
    const get = http.get as unknown as ReturnType<typeof vi.fn>;
    get.mockResolvedValue({ data: { base: 'origin/main', changed: false, diff: '' } });

    const { container, root } = await renderChanges('1');
    expect(container.textContent).toContain('Изменений нет');

    act(() => root.unmount());
  });

  it('показывает изменения файлов против дефолтной ветки: дифф и базу', async () => {
    const get = http.get as unknown as ReturnType<typeof vi.fn>;
    get.mockResolvedValue({
      data: {
        base: 'origin/main',
        changed: true,
        diff: [
          'diff --git a/README.md b/README.md',
          '--- a/README.md',
          '+++ b/README.md',
          '@@ -1 +1 @@',
          '-старое',
          '+новое',
          'diff --git a/new.txt b/new.txt',
          'new file mode 100644',
          '--- /dev/null',
          '+++ b/new.txt',
          '+свежий файл',
        ].join('\n'),
      },
    });

    const { container, root } = await renderChanges('1');
    const text = container.textContent ?? '';
    expect(text).toContain('Сравнение с origin/main');
    expect(text).toContain('README.md');
    expect(text).toContain('-старое');
    expect(text).toContain('+новое');
    expect(text).toContain('+свежий файл');

    act(() => root.unmount());
  });

  it('только показывает изменения: коммитить и пушить страница не даёт', async () => {
    const get = http.get as unknown as ReturnType<typeof vi.fn>;
    get.mockResolvedValue({
      data: { base: null, changed: true, diff: 'diff --git a/x b/x\n+X\n' },
    });

    const { container, root } = await renderChanges('1');
    const text = container.textContent ?? '';
    expect(text).not.toMatch(/коммит|пушить|запушить|commit|push/i);

    // На странице нет ни кнопок, ни действий записи в git.
    const source = readFileSync('src/pages/app/changes.tsx', 'utf-8');
    expect(source).not.toMatch(/commit|push|закоммит/i);
    expect(source).not.toMatch(/\.post\(|action\(/);

    act(() => root.unmount());
  });
});
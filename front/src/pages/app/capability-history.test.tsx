import { describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { createRoot, type Root } from 'react-dom/client';

vi.mock('@/services/http', () => ({
  default: { get: vi.fn() },
}));

import http from '@/services/http';
import CapabilityHistoryPage from '@/pages/app/capabilityHistory';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const entries = [
  { id: 1, action: 'create', actor_id: 2, actor_name: 'alice', created_at: '2026-09-01T10:00:00Z', detail: null, content: 'v1' },
  { id: 2, action: 'update', actor_id: 2, actor_name: 'alice', created_at: '2026-09-01T11:00:00Z', detail: null, content: 'v2' },
  { id: 3, action: 'rename', actor_id: 3, actor_name: 'bob', created_at: '2026-09-01T12:00:00Z', detail: 'review2', content: 'v2' },
  { id: 4, action: 'delete', actor_id: 4, actor_name: 'carol', created_at: '2026-09-01T13:00:00Z', detail: null, content: 'v2' },
];

async function renderHistory() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => {
    root.render(
      <MemoryRouter initialEntries={['/skills/1/history']}>
        <Routes>
          <Route path="skills/:id/history" element={<CapabilityHistoryPage />} />
        </Routes>
      </MemoryRouter>,
    );
  });
  await act(async () => {});
  return { container, root };
}

describe('CapabilityHistoryPage', () => {
  it('shows changes of one record in order: who, when and what', async () => {
    const get = http.get as unknown as ReturnType<typeof vi.fn>;
    get.mockResolvedValue({ data: entries });

    const { container, root } = await renderHistory();
    const text = container.textContent ?? '';

    // Кто, когда и что сделал — все действия в порядке совершения.
    const order = ['создал', 'изменил содержимое', 'переименовал', 'удалил'];
    let last = -1;
    for (const label of order) {
      const idx = text.indexOf(label);
      expect(idx).toBeGreaterThan(last);
      last = idx;
    }
    expect(text).toContain('alice');
    expect(text).toContain('bob');
    expect(text).toContain('carol');
    expect(text).toContain('review2');

    // Дифф по соседним записям: создание показывает +v1, правка — -v1/+v2,
    // переименование и удаление содержимое не меняют (дифф пуст).
    expect(text).toContain('+v1');
    expect(text).toContain('-v1');
    expect(text).toContain('+v2');

    act(() => root.unmount());
  });
});
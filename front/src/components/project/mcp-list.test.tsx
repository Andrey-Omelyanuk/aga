import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { McpList } from './McpList';
import type { McpServer } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderList(servers: McpServer[]) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(<McpList servers={servers} onChanged={() => {}} />);
  });
  return { container, root };
}

describe('McpList', () => {
  it('shows http server with url and key, stdio server with command', () => {
    const { container, root } = renderList([
      { id: 1, name: 'github', transport: 'http', url: 'http://mcp/gh', command: '', api_key: 'k1' },
      { id: 2, name: 'fs', transport: 'stdio', url: '', command: 'mcp-fs .', api_key: null },
    ] as McpServer[]);
    const text = container.textContent ?? '';
    expect(text).toContain('github');
    expect(text).toContain('URL: http://mcp/gh');
    expect(text).toContain('Ключ: k1');
    expect(text).toContain('Команда: mcp-fs .');
    // Проверить можно только http-сервер: у stdio кнопки нет.
    const checks = Array.from(container.querySelectorAll('button')).filter(
      (b) => b.textContent === 'Проверить',
    );
    expect(checks).toHaveLength(1);
    act(() => root.unmount());
  });

  it('form switches between url and command by transport', () => {
    const { container, root } = renderList([]);
    expect(container.textContent).toContain('MCP-серверов пока нет');
    const url = () => container.querySelector('input[placeholder^="URL"]');
    const command = () => container.querySelector('input[placeholder^="Команда"]');
    expect(url()).not.toBeNull();
    expect(command()).toBeNull();
    const select = container.querySelector('select') as HTMLSelectElement;
    act(() => {
      select.value = 'stdio';
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
    expect(url()).toBeNull();
    expect(command()).not.toBeNull();
    act(() => root.unmount());
  });
});

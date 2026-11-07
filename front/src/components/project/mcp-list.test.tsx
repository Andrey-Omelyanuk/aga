import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { McpList } from './McpList';
import type { McpServer } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderList(servers: McpServer[], canEdit = true) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(<McpList servers={servers} onChanged={() => {}} canEdit={canEdit} />);
  });
  return { container, root };
}

describe('McpList', () => {
  it('shows http server with url and key presence, stdio server with command', () => {
    const { container, root } = renderList([
      { id: 1, name: 'github', transport: 'http', url: 'http://mcp/gh', command: '', has_api_key: true },
      { id: 2, name: 'fs', transport: 'stdio', url: '', command: 'mcp-fs .', has_api_key: false },
    ] as McpServer[]);
    const text = container.textContent ?? '';
    expect(text).toContain('github');
    expect(text).toContain('config.url http://mcp/gh');
    expect(text).toContain('config.key config.keySet');
    expect(text).toContain('config.command mcp-fs .');
    // Проверить можно только http-сервер: у stdio кнопки нет.
    const checks = Array.from(container.querySelectorAll('button')).filter(
      (b) => b.textContent === 'config.check',
    );
    expect(checks).toHaveLength(1);
    act(() => root.unmount());
  });

  it('form switches between url and command by transport', () => {
    const { container, root } = renderList([]);
    expect(container.textContent).toContain('config.noMcpServers');
    const url = () => container.querySelector('input[placeholder="config.urlPlaceholder"]');
    const command = () => container.querySelector('input[placeholder="config.commandPlaceholder"]');
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

  it('participant sees servers read-only but can still check http server', () => {
    const { container, root } = renderList(
      [{ id: 1, name: 'github', transport: 'http', url: 'http://mcp/gh', command: '', has_api_key: false }] as McpServer[],
      false,
    );
    expect(container.textContent).toContain('config.adminOnly');
    expect(container.querySelector('input')).toBeNull();
    const buttons = Array.from(container.querySelectorAll('button')).map((b) => b.textContent);
    expect(buttons).toEqual(['config.check']);
    act(() => root.unmount());
  });
});

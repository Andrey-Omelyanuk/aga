import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { fileBrowser } from '@/models/files';
import { FileViewer } from '@/components/files/FileViewer';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function renderViewer() {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  await act(async () => {
    root.render(<FileViewer />);
  });
  return { container, root };
}

describe('FileViewer', () => {
  it('показывает текстовый файл с подсветкой синтаксиса по языку его расширения', async () => {
    fileBrowser.currentPath = 'main.rs';
    fileBrowser.content = { contentType: 'text/plain', text: 'fn main() { let x = 1; }' };

    const { container, root } = await renderViewer();
    const code = container.querySelector('code.hljs');
    expect(code).toBeTruthy();
    expect(code?.innerHTML).toContain('hljs-');

    act(() => root.unmount());
  });

  it('показывает markdown-файл как исходник с подсветкой, а не готовым документом', async () => {
    fileBrowser.currentPath = 'README.md';
    fileBrowser.content = { contentType: 'text/plain', text: '# Заголовок\n\n| a | b |' };

    const { container, root } = await renderViewer();
    expect(container.textContent).toContain('# Заголовок');
    expect(container.querySelector('h1')).toBeNull();
    expect(container.querySelector('table')).toBeNull();
    expect(container.querySelector('code.hljs')?.innerHTML).toContain('hljs-');

    act(() => root.unmount());
  });

  it('показывает файл с неизвестным расширением без подсветки, обычным текстом', async () => {
    fileBrowser.currentPath = 'file.unknownext';
    fileBrowser.content = { contentType: 'text/plain', text: 'просто текст <b>без подсветки</b>' };

    const { container, root } = await renderViewer();
    expect(container.textContent).toContain('просто текст <b>без подсветки</b>');
    expect(container.innerHTML).not.toContain('hljs-');

    act(() => root.unmount());
  });
});
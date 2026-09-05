import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { LlmList } from './LlmList';
import type { Llm } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderList(connections: Llm[]) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(<LlmList connections={connections} onChanged={() => {}} />);
  });
  return { container, root };
}

describe('LlmList', () => {
  it('shows created connections: name, url API and access key', () => {
    const connections = [
      {
        id: 1,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        api_key: 'secret-key',
      },
      {
        id: 2,
        name: 'vllm-cluster',
        api_url: 'http://vllm:8000/v1',
        api_key: null,
      },
    ] as Llm[];

    const { container, root } = renderList(connections);
    const text = container.textContent ?? '';

    expect(text).toContain('ollama-local');
    expect(text).toContain('http://llm:11434/v1');
    expect(text).toContain('secret-key');
    expect(text).toContain('vllm-cluster');
    expect(text).toContain('http://vllm:8000/v1');

    act(() => root.unmount());
  });
});
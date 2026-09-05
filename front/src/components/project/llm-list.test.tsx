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
  it('shows created connections: name, url API, access key and model', () => {
    const connections = [
      {
        id: 1,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        api_key: 'secret-key',
        model_name: 'qwen3:0.6b',
        is_default: true,
      },
      {
        id: 2,
        name: 'vllm-cluster',
        api_url: 'http://vllm:8000/v1',
        api_key: null,
        model_name: 'qwen2.5:7b',
        is_default: false,
      },
    ] as Llm[];

    const { container, root } = renderList(connections);
    const text = container.textContent ?? '';

    expect(text).toContain('ollama-local');
    expect(text).toContain('http://llm:11434/v1');
    expect(text).toContain('secret-key');
    expect(text).toContain('qwen3:0.6b');
    expect(text).toContain('дефолтная');
    expect(text).toContain('vllm-cluster');
    expect(text).toContain('http://vllm:8000/v1');
    expect(text).toContain('qwen2.5:7b');

    act(() => root.unmount());
  });

  it('default LLM form offers connections and an empty choice', () => {
    const connections = [
      {
        id: 1,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        api_key: null,
        model_name: 'qwen3:0.6b',
        is_default: true,
      },
    ] as Llm[];

    const { container, root } = renderList(connections);
    const select = container.querySelector('select');
    expect(select).not.toBeNull();
    expect((select as HTMLSelectElement).value).toBe('1');
    const options = Array.from(select?.querySelectorAll('option') ?? []).map(
      (o) => o.textContent,
    );
    expect(options).toContain('ollama-local');
    expect(options).toContain('— нет —');

    act(() => root.unmount());
  });
});
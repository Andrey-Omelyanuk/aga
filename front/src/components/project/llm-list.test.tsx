import { describe, expect, it } from 'vitest';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { LlmList, keyPatch } from './LlmList';
import type { Llm } from '@/models/project';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function renderList(connections: Llm[], canEdit = true) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  act(() => {
    root.render(<LlmList connections={connections} onChanged={() => {}} canEdit={canEdit} />);
  });
  return { container, root };
}

describe('LlmList', () => {
  it('shows created connections: name, url API, key presence (not the key) and model', () => {
    const connections = [
      {
        id: 1,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        has_api_key: true,
        model_name: 'qwen3:0.6b',
        is_default: true,
        native_tools: true,
      },
      {
        id: 2,
        name: 'vllm-cluster',
        api_url: 'http://vllm:8000/v1',
        has_api_key: false,
        model_name: 'qwen2.5:7b',
        is_default: false,
        native_tools: false,
      },
    ] as Llm[];

    const { container, root } = renderList(connections);
    const text = container.textContent ?? '';

    expect(text).toContain('ollama-local');
    expect(text).toContain('http://llm:11434/v1');
    // Ключ write-only: виден только признак «задан».
    expect(text).toContain('config.key config.keySet');
    expect(text).toContain('qwen3:0.6b');
    expect(text).toContain('config.default');
    expect(text).toContain('vllm-cluster');
    expect(text).toContain('http://vllm:8000/v1');
    expect(text).toContain('qwen2.5:7b');
    // Режим инструментов: нативный tool calling или запасной текстовый.
    expect(text).toContain('config.tools');
    expect(text).toContain('config.toolCalling');
    expect(text).toContain('config.textMode');

    act(() => root.unmount());
  });

  it('default LLM form offers connections and an empty choice', () => {
    const connections = [
      {
        id: 1,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        has_api_key: false,
        model_name: 'qwen3:0.6b',
        is_default: true,
        native_tools: true,
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
    expect(options).toContain('config.none');

    act(() => root.unmount());
  });

  it('participant sees connections read-only: no form, edit or delete', () => {
    const connections = [
      {
        id: 1,
        name: 'ollama-local',
        api_url: 'http://llm:11434/v1',
        has_api_key: true,
        model_name: 'qwen3:0.6b',
        is_default: true,
        native_tools: true,
      },
    ] as Llm[];
    const { container, root } = renderList(connections, false);
    const text = container.textContent ?? '';
    expect(text).toContain('ollama-local');
    expect(text).toContain('config.adminOnly');
    const buttons = Array.from(container.querySelectorAll('button')).map((b) => b.textContent);
    expect(buttons).not.toContain('config.edit');
    expect(buttons).not.toContain('config.delete');
    expect(buttons).not.toContain('config.createConnection');
    expect((container.querySelector('select') as HTMLSelectElement).disabled).toBe(true);
    act(() => root.unmount());
  });

  it('edit payload keeps, replaces or clears the hidden key', () => {
    expect(keyPatch({ api_key: '', clear_key: false })).toEqual({});
    expect(keyPatch({ api_key: '  ', clear_key: false })).toEqual({});
    expect(keyPatch({ api_key: ' new ', clear_key: false })).toEqual({ api_key: 'new' });
    expect(keyPatch({ api_key: 'new', clear_key: true })).toEqual({ api_key: '' });
  });
});

import type { Meta, StoryObj } from '@storybook/react';
import { LlmList } from './LlmList';
import type { Llm } from '@/models/project';

const connections: Llm[] = [
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

const meta = {
  title: 'project/LlmList',
  component: LlmList,
  args: {
    connections,
    onChanged: () => {},
  },
} satisfies Meta<typeof LlmList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Connections: Story = {};
import type { Meta, StoryObj } from '@storybook/react';
import { AgentSetEditor } from './AgentSetEditor';
import type { Agent, CatalogItem } from '@/models/project';

const agents: Agent[] = [
  {
    id: 10,
    name: 'src',
    description: 'Правила разработки',
    tools: ['git', 'make'],
    max_iterations: 3,
    temperature: 0.7,
    parent_id: null,
    skills: [{ name: 'review' }],
    commands: [],
    territory: { folder: 'src', excludes: ['src/backend'] },
  },
  {
    id: 11,
    name: 'src/backend',
    description: 'Правила бэкенда',
    tools: ['docker'],
    max_iterations: 3,
    temperature: 0.7,
    parent_id: 10,
    skills: [],
    commands: [{ name: 'deploy' }],
    territory: { folder: 'src/backend', excludes: [] },
  },
];

const skills: CatalogItem[] = [
  {
    id: 1,
    name: 'review',
    content: 'Проверять диф и тесты',
    deleted: false,
  },
];

const commands: CatalogItem[] = [
  { id: 1, name: 'deploy', content: 'Выкатывать', deleted: false },
];

const meta = {
  title: 'project/AgentSetEditor',
  component: AgentSetEditor,
  args: {
    setId: 1,
    name: 'ops',
    agents,
    skills,
    commands,
    onSaved: () => {},
  },
} satisfies Meta<typeof AgentSetEditor>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Composition: Story = {};
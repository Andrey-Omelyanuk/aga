import type { Meta, StoryObj } from '@storybook/react';
import { MemoryRouter } from 'react-router-dom';
import { CapabilityEditor } from './CapabilityEditor';
import type { CatalogItem } from '@/models/project';

const items: CatalogItem[] = [
  {
    id: 1,
    name: 'code-review',
    content: 'Ревью по чеклисту: архитектура, безопасность, тесты.',
    deleted: false,
  },
  {
    id: 2,
    name: 'git-workflow',
    content: 'Ветки feature/*, коммиты по conventional-commits.',
    deleted: false,
  },
];

const deleted: CatalogItem[] = [
  { id: 3, name: 'legacy-skill', content: '', deleted: true },
];

const meta = {
  title: 'project/CapabilityEditor',
  component: CapabilityEditor,
  decorators: [
    (Story) => (
      <MemoryRouter>
        <Story />
      </MemoryRouter>
    ),
  ],
  args: {
    kind: 'skills',
    items,
    deleted,
    onChanged: () => {},
  },
} satisfies Meta<typeof CapabilityEditor>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Skills: Story = {};
export const Commands: Story = {
  args: {
    kind: 'commands',
    items: [
      {
        id: 4,
        name: 'run-tests',
        content: 'Запуск юнит-тестов и линтера.',
        deleted: false,
      },
    ],
  },
};

import type { Meta, StoryObj } from '@storybook/react';
import { CapabilityEditor } from '@/components/project/CapabilityEditor';
import type { CatalogItem } from '@/models/project';

const skills: CatalogItem[] = [
  {
    id: 1,
    name: 'review',
    versions: [
      { version: '1', content: 'Проверять диф' },
      { version: '2', content: 'Проверять диф и тесты' },
    ],
  },
  { id: 2, name: 'polish', versions: [] },
];

const meta = {
  title: 'project/CapabilityEditor',
  component: CapabilityEditor,
} satisfies Meta<typeof CapabilityEditor>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Skills: Story = {
  args: {
    kind: 'skills',
    items: skills,
    onChanged: () => {},
  },
};

export const CommandsEmpty: Story = {
  args: {
    kind: 'commands',
    items: [],
    onChanged: () => {},
  },
};
import { useState } from 'react';
import type { Meta, StoryObj } from '@storybook/react';
import { Tabs } from '@/components/ui/tabs';

function TabsDemo() {
  const [value, setValue] = useState<string>('projects');
  const tabs: Array<{ value: string; label: string }> = [
    { value: 'projects', label: 'Проекты' },
    { value: 'chat', label: 'Чат' },
    { value: 'files', label: 'Файлы' },
  ];
  return <Tabs tabs={tabs} value={value} onChange={setValue} />;
}

const meta = {
  title: 'ui/Tabs',
  component: TabsDemo,
} satisfies Meta<typeof TabsDemo>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
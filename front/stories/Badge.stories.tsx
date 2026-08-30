import type { Meta, StoryObj } from '@storybook/react';
import { Badge } from '@/components/ui/badge';

const meta = {
  title: 'ui/Badge',
  component: Badge,
  args: { children: 'ready' },
} satisfies Meta<typeof Badge>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Ok: Story = { args: { variant: 'ok' } };
export const Warn: Story = { args: { variant: 'warn' } };
export const Info: Story = { args: { variant: 'info' } };
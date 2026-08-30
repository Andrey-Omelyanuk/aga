import type { Meta, StoryObj } from '@storybook/react';
import { Button } from '@/components/ui/button';

const meta = {
  title: 'ui/Button',
  component: Button,
  args: { children: 'Кнопка' },
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: { variant: 'primary' },
};

export const Secondary: Story = {
  args: { variant: 'secondary' },
};

export const Outline: Story = {
  args: { variant: 'outline' },
};

export const Disabled: Story = {
  args: { disabled: true },
};
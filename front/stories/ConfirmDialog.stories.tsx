import type { Meta, StoryObj } from '@storybook/react';
import { ConfirmDialog } from '@/components/ui/ConfirmDialog';

const meta = {
  title: 'ui/ConfirmDialog',
  component: ConfirmDialog,
  args: {
    title: 'Удалить способность?',
    message: '«review» будет удалена, но её история сохранится.',
    onConfirm: () => {},
    onCancel: () => {},
  },
} satisfies Meta<typeof ConfirmDialog>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Danger: Story = {};
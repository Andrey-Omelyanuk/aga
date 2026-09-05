import type { Meta, StoryObj } from '@storybook/react';
import { HelpContent } from './HelpContent';

const meta = {
  title: 'settings/HelpContent',
  component: HelpContent,
} satisfies Meta<typeof HelpContent>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Russian: Story = {
  args: { lang: 'ru' },
};

export const English: Story = {
  args: { lang: 'en' },
};
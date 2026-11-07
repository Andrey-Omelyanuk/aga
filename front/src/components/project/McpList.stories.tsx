import type { Meta, StoryObj } from '@storybook/react';
import { McpList } from './McpList';
import type { McpServer } from '@/models/project';

const servers: McpServer[] = [
  {
    id: 1,
    name: 'github',
    transport: 'http',
    url: 'https://api.githubcopilot.com/mcp/',
    command: '',
    has_api_key: true,
  },
  {
    id: 2,
    name: 'fs',
    transport: 'stdio',
    url: '',
    command: 'npx -y @modelcontextprotocol/server-filesystem .',
    has_api_key: false,
  },
] as McpServer[];

const meta = {
  title: 'project/McpList',
  component: McpList,
  args: {
    servers,
    onChanged: () => {},
    canEdit: true,
  },
} satisfies Meta<typeof McpList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Servers: Story = {};

/** Участник (не админ): только просмотр и проверка http-сервера. */
export const ReadOnly: Story = { args: { canEdit: false } };

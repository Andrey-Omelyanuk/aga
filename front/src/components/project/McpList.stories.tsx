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
    api_key: 'ghp-secret',
  },
  {
    id: 2,
    name: 'fs',
    transport: 'stdio',
    url: '',
    command: 'npx -y @modelcontextprotocol/server-filesystem .',
    api_key: null,
  },
] as McpServer[];

const meta = {
  title: 'project/McpList',
  component: McpList,
  args: {
    servers,
    onChanged: () => {},
  },
} satisfies Meta<typeof McpList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Servers: Story = {};

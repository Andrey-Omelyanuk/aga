import type { Meta, StoryObj } from '@storybook/react';
import { SshKeyPanel } from '@/components/settings/SshKeyPanel';

const meta = {
  title: 'settings/SshKeyPanel',
  component: SshKeyPanel,
} satisfies Meta<typeof SshKeyPanel>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Configured: Story = {
  args: {
    info: {
      configured: true,
      public_key:
        'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIA3Xx6zmo62u22+R1RFiQdTtrlrgb25kAFtqD1mLc9/sV aga',
      fingerprint: 'SHA256:FwcCaoMSAprJYBM6MG70L5SQqVrUH7zpbY6IbIBDmaI',
    },
  },
};

export const NotConfigured: Story = {
  args: {
    info: { configured: false, public_key: null, fingerprint: null },
  },
};
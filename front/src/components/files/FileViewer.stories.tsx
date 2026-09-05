import type { Meta, StoryObj } from '@storybook/react';
import { FileViewer } from './FileViewer';
import { fileBrowser } from '@/models/files';

const setContent = (path: string, text: string) => {
  fileBrowser.currentPath = path;
  fileBrowser.content = { contentType: 'text/plain', text };
  fileBrowser.loading = false;
};

const meta = {
  title: 'files/FileViewer',
  component: FileViewer,
} satisfies Meta<typeof FileViewer>;

export default meta;
type Story = StoryObj<typeof meta>;

export const RustCode: Story = {
  render: () => {
    setContent('main.rs', 'fn main() {\n    let answer = 42;\n    println!("{answer}");\n}');
    return <FileViewer />;
  },
};

export const MarkdownSource: Story = {
  render: () => {
    setContent('README.md', '# Заголовок\n\n- пункт\n\n```rust\nfn main() {}\n```');
    return <FileViewer />;
  },
};

export const PlainText: Story = {
  render: () => {
    setContent('notes.txt', 'обычный текст без подсветки');
    return <FileViewer />;
  },
};

export const Empty: Story = {
  render: () => {
    fileBrowser.currentPath = null;
    fileBrowser.content = null;
    return <FileViewer />;
  },
};
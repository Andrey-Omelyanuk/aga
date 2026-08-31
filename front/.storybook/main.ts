import type { StorybookConfig } from '@storybook/react-vite';

const config: StorybookConfig = {
  stories: ['../stories/**/*.stories.tsx', '../src/**/*.stories.tsx'],
  addons: ['@storybook/addon-links', '@storybook/addon-essentials'],
  framework: {
    name: '@storybook/react-vite',
    options: {},
  },
  docs: {
    autodocs: 'tag',
  },
  // react-docgen падает на моделях с декораторами (@api/@model из mobx-model-ui) —
  // таблицы пропсов не генерируем, витрина компонентов остаётся.
  typescript: {
    reactDocgen: false,
  },
};

export default config;
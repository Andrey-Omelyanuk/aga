import { vi } from 'vitest';

// Mock react-i18next to return translation keys instead of actual translations
// This ensures tests verify that components use the correct keys without depending on translation files
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: {
      language: 'en',
      changeLanguage: vi.fn(),
    },
  }),
  initReactI18next: {
    type: '3rdParty',
    init: vi.fn(),
  },
}));

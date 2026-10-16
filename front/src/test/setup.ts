import { vi } from 'vitest';
import i18n from '@/i18n/config';

// Mock react-i18next to use the real i18n instance
// This ensures tests can verify language switching behavior
vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n,
  }),
  initReactI18next: {
    type: '3rdParty',
    init: vi.fn(),
  },
}));

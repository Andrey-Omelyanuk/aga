import { defineConfig, devices } from '@playwright/test';

// E2E через веб-клиент dev-стенда (http://dev.localhost). Стенд готовит
// e2e/run.sh (сид, mock-LLM), браузер ходит через прокси *.localhost как
// пользователь. Описание сценариев — e2e/README.md.
export default defineConfig({
  testDir: './tests',
  // Сценарии — один путь пользователя на общем стенде: строго по очереди.
  workers: 1,
  fullyParallel: false,
  retries: 0,
  // Ответ маленькой LLM (ollama) бывает долгим.
  timeout: 15 * 60_000,
  expect: { timeout: 30_000 },
  reporter: [['list'], ['html', { open: 'never' }]],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://dev.localhost',
    locale: 'ru-RU',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    video: 'retain-on-failure',
  },
  projects: [
    // Вход через Keycloak — сам по себе тест SSO; сохраняет сессии alice и bob.
    { name: 'login', testMatch: /login\.setup\.ts/ },
    {
      name: 'e2e',
      testMatch: /\.spec\.ts/,
      dependencies: ['login'],
      use: { ...devices['Desktop Chrome'] },
    },
  ],
});

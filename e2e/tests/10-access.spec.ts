import { expect, test } from '@playwright/test';
import { as, card, loginViaKeycloak, profileLink } from './lib/ui';

// Доступ и сессия входа в браузере: роль из Keycloak ограничивает действия,
// просроченный токен обновляется молча, выход возвращает на экран входа.

test('participant (alice) cannot release a workstation — the UI says so', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  await alice.goto('/workstations');
  const station = card(alice, /^ws-2\b/); // сид: ws-2 свободна
  await station.getByRole('button', { name: 'Отпустить' }).click();
  await expect(alice.getByText('Недостаточно прав')).toBeVisible();
});

test('expired access token is refreshed silently — no login screen', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  await alice.goto('/projects');
  await expect(profileLink(alice)).toContainText('alice');

  // Токен доступа «протух»: ядро ответит 401, SPA обновит его по refresh-токену.
  await alice.evaluate(() => localStorage.setItem('aga_token', 'expired.token.value'));
  await alice.reload();
  await expect(profileLink(alice)).toContainText('alice');
  await expect(alice.getByRole('button', { name: 'Войти через SSO' })).toHaveCount(0);
  expect(await alice.evaluate(() => localStorage.getItem('aga_token'))).not.toBe('expired.token.value');
});

test('logout ends the SSO session: back to the login screen, next login asks for the password', async ({ browser }) => {
  // Отдельный вход: выход завершает SSO-сессию Keycloak, общие сессии
  // остальных тестов трогать нельзя.
  const page = await (await browser.newContext()).newPage();
  await loginViaKeycloak(page, 'alice');

  await profileLink(page).click();
  await page.getByRole('button', { name: 'Выйти' }).click();
  // Keycloak просит подтвердить выход (ядро не передаёт id_token_hint) —
  // подтверждаем, как человек, и возвращаемся в SPA.
  await expect(page).toHaveURL(/openid-connect\/logout/);
  await page.locator('#kc-logout').click();
  await expect(page.getByRole('button', { name: 'Войти через SSO' })).toBeVisible();
  await expect(profileLink(page)).toHaveCount(0);

  // SSO-сессии больше нет: вход снова показывает форму с паролем.
  await page.getByRole('button', { name: 'Войти через SSO' }).click();
  await expect(page.locator('#password')).toBeVisible();
});

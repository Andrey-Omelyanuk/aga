import { expect, test as setup } from '@playwright/test';
import { USERS, authFile, type UserName } from './lib/stand';
import { loginViaKeycloak, profileLink } from './lib/ui';

// Вход через SSO: аноним видит только экран входа, кнопка ведёт в форму
// Keycloak, после входа SPA знает пользователя и его роль. Сессии alice и bob
// сохраняются — остальные тесты стартуют уже вошедшими.

setup('anonymous sees only the login screen', async ({ page }) => {
  await page.goto('/projects');
  await expect(page.getByRole('button', { name: 'Войти через SSO' })).toBeVisible();
  await expect(page.getByRole('navigation')).toHaveCount(0);
});

for (const user of Object.keys(USERS) as UserName[]) {
  setup(`${user} logs in through the Keycloak form`, async ({ page }) => {
    await loginViaKeycloak(page, user);
    // Роль приходит из токена Keycloak: admin — только у bob.
    if (user === 'bob') await expect(profileLink(page)).toContainText('admin');
    else await expect(profileLink(page)).not.toContainText('admin');
    await page.context().storageState({ path: authFile(user) });
  });
}

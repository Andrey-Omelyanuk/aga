import { expect, test } from '@playwright/test';
import { PROJECT_GIT_URL, api } from './lib/stand';
import { as, card, wsSelect } from './lib/ui';

// Станция упала: админ помечает ws-1 как down — UI это показывает, станция не
// предлагается для сессий, работа над проектом переезжает на ws-2. Идёт
// последним: после него ws-1 до следующего сида недоступна.

test('workstation down: marked in the UI, not offered, the project moves to another station', async ({ browser }) => {
  // Пометить станцию упавшей в веб-клиенте нельзя — только API админа.
  const ws1 = (await api<any[]>('bob', 'GET', '/workstations')).find((w) => w.name === 'ws-1');
  await api('bob', 'POST', `/workstations/${ws1.id}/down`);

  const alice = await as(browser, 'alice');
  await alice.goto('/workstations');
  await expect(card(alice, /^ws-1\b/)).toContainText('down');

  // Для новой сессии ws-1 не предлагается — только ws-2.
  await alice.goto('/sessions');
  await alice.locator('header select').selectOption({ label: PROJECT_GIT_URL });
  await expect(wsSelect(alice).locator('option', { hasText: 'ws-1' })).toHaveCount(0);
  await wsSelect(alice).selectOption({ label: 'ws-2 (ready)' });
  await alice.getByPlaceholder('Название сессии').fill('e2e: после падения ws-1');
  await alice.getByRole('button', { name: 'Открыть сессию' }).click();
  await expect(alice).toHaveURL(/\/chat\/\d+/);

  // Проект развёрнут на ws-2.
  await alice.goto('/files');
  const readme = alice.getByRole('button', { name: 'README.md' });
  await expect(async () => {
    await wsSelect(alice).selectOption({ label: 'ws-2 (ready)' });
    await expect(readme).toBeVisible({ timeout: 5_000 });
  }).toPass({ timeout: 120_000 });
});

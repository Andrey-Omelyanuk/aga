import { expect, test, type Locator, type Page } from '@playwright/test';
import { as, closestWith } from './lib/ui';

// Каталог скиллов и его история: alice и bob по очереди меняют один скилл,
// история показывает каждое действие с автором — и остаётся после удаления.
const run = Date.now();
const NAME = `e2e-skill-${run}`;
const RENAMED = `e2e-skill-renamed-${run}`;

/** Строка скилла в списке слева: кнопка с именем, «История», «Удалить». */
function row(page: Page, name: string): Locator {
  return page.getByRole('button', { name, exact: true }).locator('..');
}

async function save(page: Page) {
  await page.getByRole('button', { name: 'Сохранить', exact: true }).click();
  await expect(page.getByText('Изменения сохранены')).toBeVisible();
}

test('skill: create, edit, rename, delete — history shows who did what', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');

  // alice создаёт скилл.
  await alice.goto('/config/skills');
  await alice.getByPlaceholder('Имя').fill(NAME);
  await alice.getByRole('button', { name: 'Создать' }).click();
  await expect(alice.getByText('Способность создана')).toBeVisible();

  // bob пишет содержимое.
  await bob.goto('/config/skills');
  await bob.getByRole('button', { name: NAME, exact: true }).click();
  await bob.getByPlaceholder('Содержимое скилла (markdown; агент берёт его всегда)').fill('Проверяй тесты (e2e).');
  await save(bob);

  // alice переименовывает, содержимое не трогает.
  await alice.reload();
  await alice.getByRole('button', { name: NAME, exact: true }).click();
  await expect(alice.getByPlaceholder('Содержимое скилла (markdown; агент берёт его всегда)')).toHaveValue(
    'Проверяй тесты (e2e).',
  );
  // Поле имени — рядом с «Сохранить» (без placeholder).
  await alice.getByRole('button', { name: 'Сохранить', exact: true }).locator('xpath=preceding-sibling::input').fill(RENAMED);
  await save(alice);

  // bob удаляет — с подтверждением; скилл уходит в «Удалённые».
  await bob.reload();
  await row(bob, RENAMED).getByRole('button', { name: 'Удалить' }).click();
  const dialog = closestWith(bob.locator('h3', { hasText: 'Удалить способность?' }), 'Удалить');
  await dialog.getByRole('button', { name: 'Удалить' }).click();
  await expect(bob.getByText('Способность удалена')).toBeVisible();
  const deleted = bob.locator('span.line-through', { hasText: RENAMED });
  await expect(deleted).toBeVisible();

  // История доступна и после удаления: действия по порядку, с авторами.
  await deleted.locator('..').getByRole('link', { name: 'История' }).click();
  await expect(bob.getByText('История изменений')).toBeVisible();
  const entries = bob.getByTestId('history-entry');
  const expected: Array<[string, string]> = [
    ['создал', 'alice'],
    ['изменил содержимое', 'bob'],
    ['переименовал', 'alice'],
    ['удалил', 'bob'],
  ];
  // Переименование без правки содержимого — только «переименовал» (без пустого
  // «изменил содержимое»).
  await expect(entries).toHaveCount(expected.length);
  for (const [i, [action, actor]] of expected.entries()) {
    await expect(entries.nth(i)).toContainText(action);
    await expect(entries.nth(i)).toContainText(actor);
  }
  await expect(entries.nth(2)).toContainText(RENAMED);
  await expect(entries.nth(1)).toContainText('Проверяй тесты (e2e).');
});

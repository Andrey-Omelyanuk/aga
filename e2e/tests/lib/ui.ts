import { expect, type Browser, type Locator, type Page } from '@playwright/test';
import { USERS, authFile, type UserName } from './stand';

/** Браузер пользователя с сохранённым входом (login.setup.ts). */
export async function as(browser: Browser, user: UserName): Promise<Page> {
  const context = await browser.newContext({ storageState: authFile(user) });
  return context.newPage();
}

/** Вход через форму Keycloak, как человек: кнопка SSO → форма → назад в SPA. */
export async function loginViaKeycloak(page: Page, user: UserName) {
  const { login, password } = USERS[user];
  await page.goto('/');
  await page.getByRole('button', { name: 'Войти через SSO' }).click();
  await expect(page).toHaveURL(/openid-connect\/auth/);
  await page.locator('#username').fill(login);
  await page.locator('#password').fill(password);
  await page.locator('#kc-login').click();
  await expect(page).toHaveURL(/\/\/dev\./);
  await expect(profileLink(page)).toContainText(login);
}

export function profileLink(page: Page): Locator {
  return page.locator('header a[title="Профиль"]');
}

/** Карточка списка (Card): заголовок h4 и его родитель. */
export function card(page: Page, title: RegExp): Locator {
  return page.getByRole('heading', { name: title }).locator('..');
}

/** Ближайший предок, содержащий кнопку с этим текстом, — форма/строка. */
export function closestWith(anchor: Locator, button: string): Locator {
  return anchor.locator(`xpath=ancestor::*[.//button[normalize-space()="${button}"]][1]`);
}

export function wsSelect(page: Page): Locator {
  return page.locator('select').filter({ has: page.locator('option', { hasText: 'Выберите воркстейшн...' }) });
}

/** Строки сообщений (data-testid="message"); фильтр по тексту. */
export function messages(page: Page, text?: string | RegExp): Locator {
  const rows = page.getByTestId('message');
  return text ? rows.filter({ hasText: text }) : rows;
}

/** Сообщения агента от имени пользователя (по умолчанию alice). Сюда же
 *  попадают шаги агента — команды shell с выводом в «▸ скрытое». */
export function agentReply(page: Page, text?: string | RegExp, author: UserName = 'alice'): Locator {
  const rows = page.locator(`[data-testid="message"][data-origin="agent"][data-author="${author}"]`);
  return text ? rows.filter({ hasText: text }) : rows;
}

/** Ответить на сообщение: «↩ ответить» → форма ответа → «Отправить». */
export async function replyTo(page: Page, message: Locator, body: string) {
  await message.getByRole('button', { name: '↩ ответить' }).click();
  const form = closestWith(page.getByPlaceholder('Ответ…'), 'Отправить');
  await form.getByPlaceholder('Ответ…').fill(body);
  await form.getByRole('button', { name: 'Отправить' }).click();
}

/** Открыть сессию «e2e: mobx-model-ui» (её открывает 40-agent-cycle). */
export async function openSessionChat(page: Page) {
  await page.goto('/sessions');
  await card(page, /^e2e: mobx-model-ui/).getByRole('button', { name: 'Открыть в чате' }).click();
  await expect(page).toHaveURL(/\/chat\/\d+/);
}

/** Открыть чат из списка слева на вкладке «Чат». */
export async function openChat(page: Page, title: string) {
  await page.goto('/chat');
  await page.getByRole('button', { name: new RegExp(`^${title}`) }).click();
  await expect(page).toHaveURL(/\/chat\/\d+/);
}

/** Сообщение в основной чат (поле ввода внизу страницы). */
export async function send(page: Page, body: string) {
  await page.getByPlaceholder('Введите сообщение...').fill(body);
  await page.getByRole('button', { name: 'Отправить', exact: true }).last().click();
  await expect(messages(page, body)).toBeVisible();
}

import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';
import { PROJECT_GIT_URL, api } from './lib/stand';
import { agentReply, as, card, closestWith, messages, send, wsSelect } from './lib/ui';

// Рабочий цикл через веб-клиент: alice и bob — два браузера на одном стенде.
// Шаги идут по очереди и продолжают друг друга: сессия из первого шага — чат
// для остальных. Описание — e2e/README.md.
test.describe.configure({ mode: 'serial' });

const SEED_SESSION = 'Сессия: backend'; // сид: сессия alice на ws-1
const WS = 'ws-1';
const SESSION_TITLE = 'e2e: mobx-model-ui';

let chatPath = '';

function fixture(name: string): any {
  return JSON.parse(readFileSync(new URL(`../fixtures/${name}`, import.meta.url), 'utf8'));
}

test('workstation: close the session, release the station, open a session with the project', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');

  // Владелец закрывает сессию сида.
  await alice.goto('/sessions');
  const seedSession = card(alice, new RegExp(`^${SEED_SESSION}`));
  await seedSession.getByRole('button', { name: 'Закрыть' }).click();
  await expect(seedSession.getByRole('button', { name: 'Закрыть' })).toHaveCount(0);

  // Станция свободна — админ её отпускает.
  await bob.goto('/workstations');
  const station = card(bob, new RegExp(`^${WS}\\b`));
  await expect(station).toContainText('Свободен');
  await station.getByRole('button', { name: 'Отпустить' }).click();
  await expect(bob.getByText(/Не удалось|Недостаточно прав|открыта сессия/)).toHaveCount(0);

  // Новая сессия: проект в шапке, станция, название → переход в чат сессии.
  await alice.goto('/sessions');
  await alice.locator('header select').selectOption({ label: PROJECT_GIT_URL });
  await wsSelect(alice).selectOption({ label: `${WS} (ready)` });
  await alice.getByPlaceholder('Название сессии').fill(SESSION_TITLE);
  await alice.getByRole('button', { name: 'Открыть сессию' }).click();
  await expect(alice).toHaveURL(/\/chat\/\d+/);
  chatPath = new URL(alice.url()).pathname;

  // Занятая станция больше не предлагается для новой сессии.
  await alice.goto('/sessions');
  await expect(wsSelect(alice).locator('option', { hasText: WS })).toHaveCount(0);
});

test('files: the cloned project is browsable, README.md opens', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  const alice = await as(browser, 'alice');
  await alice.goto('/files');
  const readme = alice.getByRole('button', { name: 'README.md' });
  await expect(async () => {
    await wsSelect(alice).selectOption({ label: `${WS} (ready)` });
    await expect(readme).toBeVisible({ timeout: 5_000 });
  }).toPass({ timeout: 120_000 });

  await readme.click();
  await expect(alice.getByText(/mobx-model-ui/i).last()).toBeVisible();
});

test('agent: bob asks @alice, the agent answers as alice live in the chat', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  const bob = await as(browser, 'bob');
  await bob.goto(chatPath);
  await send(bob, '@alice что за проект?');

  // Ответ приходит по websocket (Centrifugo) — без перезагрузки страницы.
  const reply = agentReply(bob).first();
  await expect(reply).toBeVisible({ timeout: 10 * 60_000 });
  await expect(reply).not.toContainText('Ошибка:');
  await expect(reply.getByTestId('artifact')).toBeVisible();
});

test('agent: a message without @alice does not wake the agent', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  const bob = await as(browser, 'bob');
  await bob.goto(chatPath);
  // Чат загружен — виден ответ агента из прошлого шага; считаем ответы после этого.
  await expect(agentReply(bob).first()).toBeVisible();
  const before = await agentReply(bob).count();
  await send(bob, 'заметка без упоминания');
  // Агент молчит: за 20 с новых ответов от имени alice нет.
  await bob.waitForTimeout(20_000);
  await expect(agentReply(bob)).toHaveCount(before);
});

test('setup: switch the project to a mock-LLM agent (LLM page + API)', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  // LLM-подключение — через страницу «LLM», как это делает админ.
  const conn = fixture('mock-llm-connection.json');
  const bob = await as(browser, 'bob');
  await bob.goto('/config/llms');
  await bob.getByPlaceholder('Название').fill(conn.name);
  await bob.getByPlaceholder('URL API (…/v1)').fill(conn.api_url);
  await bob.getByPlaceholder('Модель').fill(conn.model_name);
  await bob.getByRole('button', { name: 'Создать подключение' }).click();
  await expect(bob.getByText('Не удалось')).toHaveCount(0);
  const findLlm = async () => (await api<any[]>('bob', 'GET', '/llms')).find((l) => l.name === conn.name);
  await expect.poll(findLlm).toBeTruthy();

  // Привязки набора к проекту в веб-клиенте нет — набор и привязка через API.
  const llm = await findLlm();
  const alice = await api('alice', 'GET', '/users/me');
  const set = fixture('ask-human-set.json');
  set.agents[0].llm_id = llm.id;
  set.agents[0].listen_user_id = alice.id;
  const created = await api('bob', 'POST', '/agent-sets', set);
  const project = (await api<any[]>('bob', 'GET', '/projects')).find((p) => p.git_url === PROJECT_GIT_URL);
  await api('bob', 'POST', `/projects/${project.id}/agent-set`, { agent_set_id: created.id });
});

test('ask_human: the agent asks in the chat, bob answers, the agent finishes', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  const bob = await as(browser, 'bob');
  await bob.goto(chatPath);
  await send(bob, '@alice e2e: выкати прод');

  // Вопрос агента — обычное сообщение, без служебных строк.
  const question = agentReply(bob, 'Разрешить деплой на прод?');
  await expect(question).toBeVisible({ timeout: 5 * 60_000 });
  await expect(bob.getByText(/Request ID|WAITING_FOR_HUMAN/)).toHaveCount(0);

  // bob не привязан к агенту, но его ответ на вопрос возобновляет агента.
  await question.getByRole('button', { name: '↩ ответить' }).click();
  const form = closestWith(bob.getByPlaceholder('Ответ…'), 'Отправить');
  await form.getByPlaceholder('Ответ…').fill('да, катим');
  await form.getByRole('button', { name: 'Отправить' }).click();

  await expect(agentReply(bob, /Готово \(e2e mock\).*да, катим/)).toBeVisible({ timeout: 5 * 60_000 });
});

test('thread: in a thread started by alice the agent answers every message', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await alice.goto(chatPath);
  await bob.goto(chatPath);

  // alice начинает нить от сообщения bob; своё сообщение агента не будит.
  await messages(alice, 'заметка без упоминания').getByRole('button', { name: '↳ начать нить' }).click();
  await alice.getByPlaceholder('Заголовок нити').fill('Обсуждение заметки');
  await alice.getByPlaceholder('Первое сообщение нити').fill('давай обсудим');
  await alice.getByRole('button', { name: 'Начать', exact: true }).click();

  // bob пишет в нить без упоминания — агент alice отвечает в этой же нити.
  await bob.getByRole('button', { name: '▸ Обсуждение заметки' }).click();
  await bob.getByPlaceholder('Сообщение в нить…').fill('а что с тестами?');
  await bob.getByPlaceholder('Сообщение в нить…').press('Enter');
  await expect(agentReply(bob, /Готово \(e2e mock\).*а что с тестами/)).toBeVisible({ timeout: 5 * 60_000 });
});

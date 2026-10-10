import { readFileSync } from 'node:fs';
import { expect, test, type Page } from '@playwright/test';
import { PROJECT_GIT_URL, MOCK_LLM_URL, api } from './lib/stand';
import { agentReply, as, card, closestWith, messages, replyTo, send, wsSelect } from './lib/ui';

// Рабочий цикл агентов через веб-клиент: alice и bob — два браузера на одном
// стенде. Шаги идут по очереди и продолжают друг друга: сессия из первого
// шага — чат для остальных. Описание — e2e/README.md.
test.describe.configure({ mode: 'serial' });

const SEED_SESSION = 'Сессия: backend'; // сид: сессия alice на ws-1
const WS = 'ws-1';
const SESSION_TITLE = 'e2e: mobx-model-ui';
const TEAM = `e2e-team-${Date.now()}`;

let chatPath = '';

function fixture(name: string): any {
  return JSON.parse(readFileSync(new URL(`../fixtures/${name}`, import.meta.url), 'utf8'));
}

async function inSession(page: Page) {
  test.skip(!chatPath, 'нет сессии из первого шага');
  await page.goto(chatPath);
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

test('agent (ollama): bob asks @alice, the agent answers as alice live in the chat', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await inSession(bob);
  await send(bob, '@alice что за проект?');

  // Ответ приходит по websocket (Centrifugo) — без перезагрузки страницы.
  const reply = agentReply(bob).filter({ has: bob.getByTestId('artifact') }).first();
  await expect(reply).toBeVisible({ timeout: 10 * 60_000 });
  await expect(reply).not.toContainText('Ошибка:');
});

test('agent: a message without @alice does not wake the agent', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await inSession(bob);
  // Чат загружен — виден ответ агента из прошлого шага; считаем ответы после этого.
  await expect(agentReply(bob).first()).toBeVisible();
  const before = await agentReply(bob).count();
  await send(bob, 'заметка без упоминания');
  // Агент молчит: за 20 с новых сообщений от его имени нет.
  await bob.waitForTimeout(20_000);
  await expect(agentReply(bob)).toHaveCount(before);
});

test('team: bob adds a mock LLM and builds a two-agent set in the editor', async ({ browser }) => {
  test.skip(!chatPath, 'нет сессии из первого шага');
  const bob = await as(browser, 'bob');

  // LLM-подключение — через страницу «LLM».
  const conn = fixture('mock-llm-connection.json');
  // Адрес mock-LLM — контейнер инстанса (имя с префиксом aga-<user>, см. run.sh),
  // а не общий aga-llm-mock из фикстуры.
  conn.api_url = MOCK_LLM_URL;
  await bob.goto('/config/llms');
  await bob.getByPlaceholder('Название').fill(conn.name);
  await bob.getByPlaceholder('URL API (…/v1)').fill(conn.api_url);
  await bob.getByPlaceholder('Модель').fill(conn.model_name);
  await bob.getByRole('button', { name: 'Создать подключение' }).click();
  await expect(bob.getByText('Не удалось')).toHaveCount(0);

  // Набор — в редакторе: корневой агент alice (весь проект) и дочерний агент
  // bob с папкой docs (его территория — только docs/).
  await bob.goto('/config/agent-sets');
  await bob.getByPlaceholder('Имя нового набора').fill(TEAM);
  await bob.getByRole('button', { name: 'Создать набор' }).click();
  await expect(bob.getByText('Набор создан')).toBeVisible();
  await card(bob, new RegExp(`^${TEAM}`)).getByRole('button', { name: 'Редактировать' }).click();

  const team = fixture('team.json');
  const folders = bob.getByPlaceholder('папка агента (путь в проекте)');
  for (const [i, agent] of team.agents.entries()) {
    await bob.getByRole('button', { name: '+ Агент' }).click();
    await folders.nth(i).fill(agent.name);
    // Карточка агента — ближайший предок с полем правил.
    const form = folders.nth(i).locator('xpath=ancestor::*[.//textarea[@placeholder="Правила агента"]][1]');
    if (agent.parent) {
      await form.locator('select').filter({ hasText: '— нет родителя —' }).selectOption({ label: agent.parent });
    }
    await form.locator('select[title="Подключение к LLM"]').selectOption({ label: conn.name });
    await form.locator('select[title^="Пользователь чата"]').selectOption({ label: `@${agent.listen}` });
    await form.getByPlaceholder('Правила агента').fill(agent.rules);
    for (const tool of agent.tools) {
      await form.getByPlaceholder('имя инструмента').fill(tool);
      await form.getByRole('button', { name: 'Добавить' }).click();
      // Чип инструмента: имя и «×» (title «Удалить инструмент»).
      const chip = form.locator('span', { has: bob.locator('button[title="Удалить инструмент"]'), hasText: tool });
      await expect(chip).toBeVisible();
    }
  }
  await bob.getByRole('button', { name: 'Сохранить набор' }).click();
  await expect(bob.getByText('Набор сохранён')).toBeVisible();

  // После перезагрузки набор на месте: два агента.
  await bob.reload();
  const saved = card(bob, new RegExp(`^${TEAM}`));
  await expect(saved).toContainText('Агентов: 2');
  for (const agent of team.agents) await expect(saved).toContainText(agent.name);

  // Привязки набора к проекту в веб-клиенте нет — через API.
  const set = (await api<any[]>('bob', 'GET', '/agent-sets')).find((s) => s.name === TEAM);
  const project = (await api<any[]>('bob', 'GET', '/projects')).find((p) => p.git_url === PROJECT_GIT_URL);
  await api('bob', 'POST', `/projects/${project.id}/agent-set`, { agent_set_id: set.id });
});

test('work: the agent runs a command in the station — the file shows in Files and Changes', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await inSession(bob);
  await send(bob, '@alice создай файл');

  // Шаг агента — команда в чате, вывод — в «▸ скрытое»; затем финальный ответ.
  await expect(agentReply(bob, 'echo e2e-content > e2e-created.txt')).toBeVisible({ timeout: 2 * 60_000 });
  await expect(agentReply(bob, /Готово \(e2e mock\)/).last()).toBeVisible({ timeout: 2 * 60_000 });

  // «Изменения» сессии: новый файл против основной ветки.
  await bob.getByRole('link', { name: 'Изменения' }).click();
  await expect(bob.getByText('e2e-created.txt').first()).toBeVisible();
  await expect(bob.getByText('e2e-content').first()).toBeVisible();

  // «Файлы»: файл в дереве станции, содержимое открывается.
  await bob.goto('/files');
  await wsSelect(bob).selectOption({ label: `${WS} (ready)` });
  await bob.getByRole('button', { name: 'e2e-created.txt' }).click();
  await expect(bob.getByText('e2e-content').last()).toBeVisible();
});

test('work: a command outside the tool list is refused, the agent sees why', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await inSession(bob);
  await send(bob, '@alice запусти curl');
  await expect(agentReply(bob, 'Команда отклонена: `curl`').last()).toBeVisible({ timeout: 2 * 60_000 });
});

test('sandbox: the docs agent cannot write outside its folder', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  await inSession(alice);
  await send(alice, '@bob запиши вне папки');

  // Запись вне territory падает в настоящем mount namespace станции.
  await expect(agentReply(alice, /Готово \(e2e mock\).*Read-only file system/, 'bob')).toBeVisible({
    timeout: 2 * 60_000,
  });
  await alice.goto('/files');
  await wsSelect(alice).selectOption({ label: `${WS} (ready)` });
  await expect(alice.getByRole('button', { name: 'README.md' })).toBeVisible();
  await expect(alice.getByRole('button', { name: 'outside.txt' })).toHaveCount(0);
});

test('agents talk through the chat: alice’s agent asks @bob, bob’s agent answers', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await inSession(bob);
  await send(bob, '@alice спроси bob');

  // Агент alice упоминает @bob — это будит агента bob, ответ от имени bob.
  await expect(agentReply(bob, '@bob проверь, пожалуйста (e2e)')).toBeVisible({ timeout: 2 * 60_000 });
  await expect(agentReply(bob, /Готово \(e2e mock\).*проверь, пожалуйста/, 'bob')).toBeVisible({
    timeout: 2 * 60_000,
  });
});

test('ask_human: the agent asks in the chat, bob answers, the agent finishes', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await inSession(bob);
  await send(bob, '@alice e2e: выкати прод');

  // Вопрос агента — обычное сообщение, без служебных строк.
  const question = agentReply(bob, 'Разрешить деплой на прод?');
  await expect(question).toBeVisible({ timeout: 2 * 60_000 });
  await expect(bob.getByText(/Request ID|WAITING_FOR_HUMAN/)).toHaveCount(0);

  // bob не привязан к агенту alice, но его ответ на вопрос возобновляет агента.
  await replyTo(bob, question, 'да, катим');
  await expect(agentReply(bob, /Готово \(e2e mock\).*да, катим/)).toBeVisible({ timeout: 2 * 60_000 });
});

test('thread: in a thread started by alice the agent answers every message', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await inSession(alice);
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
  await expect(agentReply(bob, /Готово \(e2e mock\).*а что с тестами/)).toBeVisible({ timeout: 2 * 60_000 });
});

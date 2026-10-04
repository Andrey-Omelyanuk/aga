import { expect, test } from '@playwright/test';
import { PROJECT_GIT_URL, api } from './lib/stand';
import { agentReply, as, openSessionChat, send } from './lib/ui';

// Настоящая LLM делает работу в станции: создаёт файл командой shell.
// Включается переменной E2E_LLM_MODEL (модель ollama, например qwen3:4b):
// `E2E_LLM_MODEL=qwen3:4b make dev-e2e`. Маленькая qwen3:0.6b стенда с
// инструментами не справляется, на CPU модель 4b думает минуты.
const MODEL = process.env.E2E_LLM_MODEL;
const FILE = 'hello-e2e.txt';

test.skip(!MODEL, 'E2E_LLM_MODEL не задан — тест с настоящей LLM пропущен');

test(`real LLM (${MODEL}): the agent creates a file in the station`, async ({ browser }) => {
  test.setTimeout(30 * 60_000);

  // Подготовка через API: подключение к ollama с выбранной моделью и агент alice на нём.
  const llm = await api('bob', 'POST', '/llms', {
    name: `e2e-real-${Date.now()}`,
    api_url: 'http://ollama:11434/v1',
    model_name: MODEL,
  });
  const alice = await api('alice', 'GET', '/users/me');
  const set = await api('bob', 'POST', '/agent-sets', {
    name: `e2e-real-${Date.now()}`,
    agents: [
      {
        name: 'worker',
        description:
          'Ты агент в git-проекте. Выполняй просьбы командами через инструмент shell. ' +
          'Файл создавай командой echo с перенаправлением. Отвечай кратко.',
        tools: ['ls', 'cat'],
        max_iterations: 6,
        llm_id: llm.id,
        parent: null,
        skills: [],
        listen_user_id: alice.id,
      },
    ],
  });
  const project = (await api<any[]>('bob', 'GET', '/projects')).find((p) => p.git_url === PROJECT_GIT_URL);
  await api('bob', 'POST', `/projects/${project.id}/agent-set`, { agent_set_id: set.id });

  const bob = await as(browser, 'bob');
  await openSessionChat(bob);
  await send(bob, `@alice создай в корне проекта файл ${FILE} со строкой привет`);

  // Агент выполнил команду (шаг в чате) и ответил; файл появился в станции.
  await expect(agentReply(bob, FILE).first()).toBeVisible({ timeout: 25 * 60_000 });
  const ws = (await api<any[]>('bob', 'GET', '/workstations')).find((w) => w.name === 'ws-1');
  await expect
    .poll(async () => (await api('bob', 'GET', `/workstations/${ws.id}/tree`)).entries.map((e: any) => e.name), {
      timeout: 5 * 60_000,
    })
    .toContain(FILE);
});

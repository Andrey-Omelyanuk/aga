import { expect, test } from '@playwright/test';
import { container, docker } from './lib/stand';
import { agentReply, as, messages, openChat, openSessionChat, send } from './lib/ui';

// Устойчивость: перезапуски Centrifugo и агент-рантайма посреди работы.
// Браузер и рантайм переподключаются сами, сообщения не теряются и не
// дублируются. Нужен набор e2e-team из 40-agent-cycle (агент alice на mock).
test.describe.configure({ mode: 'serial' });
const run = Date.now();

function waitHealthy(container: string) {
  // Контейнеры без healthcheck: «running» достаточно, дальше ждут сами тесты.
  expect(docker('inspect', '-f', '{{.State.Running}}', container).trim()).toBe('true');
}

test('Centrifugo restart: browsers reconnect, people keep chatting live', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await openChat(alice, 'Общий чат');
  await openChat(bob, 'Общий чат');

  // Ждём, пока браузер bob заново подключится к Centrifugo: событие,
  // опубликованное до этого, браузер не получит (истории у канала нет).
  const reconnected = bob.waitForEvent('websocket', {
    predicate: (ws) => ws.url().includes('pub-sub'),
    timeout: 60_000,
  });
  docker('restart', container('centrifugo'));
  waitHealthy(container('centrifugo'));
  const ws = await reconnected;
  await ws.waitForEvent('framereceived', { predicate: (f) => String(f.payload).includes('"connect"') });

  await send(alice, `после рестарта Centrifugo ${run}`);
  await expect(messages(bob, `после рестарта Centrifugo ${run}`)).toBeVisible();
});

test('message sent while Centrifugo is down still reaches the agent', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await openSessionChat(bob);

  // Centrifugo лежит: ядро сохраняет сообщение, но событие никуда не уходит.
  docker('stop', container('centrifugo'));
  try {
    await send(bob, `@alice пока шина лежит ${run}`);
  } finally {
    docker('start', container('centrifugo'));
  }

  // Рантайм переподключается и догружает пропущенное из БД — агент отвечает
  // ровно один раз.
  const reply = agentReply(bob, new RegExp(`Готово \\(e2e mock\\).*пока шина лежит ${run}`));
  await expect(async () => {
    await bob.reload();
    await expect(reply).toBeVisible({ timeout: 5_000 });
  }).toPass({ timeout: 2 * 60_000 });
  await bob.waitForTimeout(5_000);
  await bob.reload();
  await expect(reply).toHaveCount(1);
});

test('agent runtime restart: the next mention is answered once, old ones are not repeated', async ({ browser }) => {
  const bob = await as(browser, 'bob');
  await openSessionChat(bob);
  await expect(agentReply(bob).first()).toBeVisible();
  const before = await agentReply(bob).count();

  docker('restart', container('agent'));
  waitHealthy(container('agent'));

  await send(bob, `@alice после рестарта агента ${run}`);
  const reply = agentReply(bob, new RegExp(`Готово \\(e2e mock\\).*после рестарта агента ${run}`));
  await expect(reply).toBeVisible({ timeout: 2 * 60_000 });
  // Старые упоминания рантайм после старта не переигрывает: новый — один ответ.
  await bob.waitForTimeout(5_000);
  await expect(agentReply(bob)).toHaveCount(before + 1);
});

// Известный пробел (см. README): сообщение, отправленное, пока процесс
// `aga agent` остановлен, рантайм после старта не обрабатывает — отметка
// старта отсекает всё, что было до запуска.
test.fixme('message sent while the agent process is down is answered after start', async () => {});

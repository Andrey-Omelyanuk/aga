import { expect, test } from '@playwright/test';
import { as, closestWith, messages, openChat, send } from './lib/ui';

// Чат между людьми в двух браузерах — «Общий чат» сида (bob и alice, без
// проекта: агенты здесь не отвечают). Всё, что пишет один, другой видит без
// перезагрузки — через websocket Centrifugo.
const CHAT = 'Общий чат';
const run = Date.now(); // уникальные тексты: повторный прогон без сида не спутает сообщения

test('message from alice reaches bob live', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await openChat(alice, CHAT);
  await openChat(bob, CHAT);

  await send(alice, `привет от alice ${run}`);
  const received = messages(bob, `привет от alice ${run}`);
  await expect(received).toBeVisible();
  await expect(received).toHaveAttribute('data-author', 'alice');
  await expect(received).toHaveAttribute('data-origin', 'user');
});

test('reply to a message reaches the other side', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await openChat(alice, CHAT);
  await openChat(bob, CHAT);

  await send(alice, `вопрос alice ${run}`);
  const question = messages(bob, `вопрос alice ${run}`);
  await question.getByRole('button', { name: '↩ ответить' }).click();
  const form = closestWith(bob.getByPlaceholder('Ответ…'), 'Отправить');
  await form.getByPlaceholder('Ответ…').fill(`ответ bob ${run}`);
  await form.getByRole('button', { name: 'Отправить' }).click();

  await expect(messages(alice, `ответ bob ${run}`)).toHaveAttribute('data-author', 'bob');
});

test('thread: alice starts it, bob writes there, bob sends a message to the parent', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await openChat(alice, CHAT);
  await openChat(bob, CHAT);

  // alice начинает нить от своего сообщения: заголовок + первое сообщение.
  await send(alice, `тема для нити ${run}`);
  await messages(alice, `тема для нити ${run}`).getByRole('button', { name: '↳ начать нить' }).click();
  await alice.getByPlaceholder('Заголовок нити').fill(`Нить ${run}`);
  await alice.getByPlaceholder('Первое сообщение нити').fill(`начало нити ${run}`);
  await alice.getByRole('button', { name: 'Начать', exact: true }).click();

  // У bob нить свёрнута у сообщения; раскрыл — пишет в неё.
  const toggle = bob.getByRole('button', { name: `▸ Нить ${run}` });
  await expect(toggle).toBeVisible();
  await toggle.click();
  await expect(messages(bob, `начало нити ${run}`)).toBeVisible();
  await bob.getByPlaceholder('Сообщение в нить…').fill(`bob в нити ${run}`);
  await bob.getByPlaceholder('Сообщение в нить…').press('Enter');

  // alice видит сообщение bob, раскрыв нить.
  await alice.getByRole('button', { name: `▸ Нить ${run}` }).click();
  await expect(messages(alice, `bob в нити ${run}`)).toHaveAttribute('data-author', 'bob');

  // bob отправляет своё сообщение из нити в родительский чат — у alice копия
  // со ссылкой на исходное сообщение.
  await messages(bob, `bob в нити ${run}`).first().getByRole('button', { name: '→ в родителя' }).click();
  const copy = messages(alice, `bob в нити ${run}`).filter({ hasText: 'из нити от сообщения' });
  await expect(copy).toBeVisible();
});

test('shortcut /review adds the hidden note to the message', async ({ browser }) => {
  const alice = await as(browser, 'alice');
  const bob = await as(browser, 'bob');
  await openChat(alice, CHAT);
  await openChat(bob, CHAT);

  // Сокращение review — из сида: «Проверь дифф и прогони тесты…».
  await send(alice, `глянь фикс ${run} /review`);
  const msg = messages(bob, `глянь фикс ${run}`);
  await msg.getByRole('button', { name: '▸ скрытое' }).click();
  await expect(msg).toContainText('Проверь дифф и прогони тесты');
});

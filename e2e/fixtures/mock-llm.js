// Детерминированная mock-LLM (OpenAI-compatible) для e2e. Ответ зависит только
// от запроса: последнего сообщения и последнего сообщения пользователя.
//
//   последнее — результат инструмента → финал «Готово (e2e mock): <вывод>»;
//   «выкати прод»       → ask_human «Разрешить деплой на прод?»;
//   «создай файл»       → shell: echo e2e-content > e2e-created.txt;
//   «запусти curl»      → shell: curl … (нет в списке инструментов — отказ);
//   «запиши вне папки»  → shell: echo … > ../outside.txt (вне территории);
//   «спроси bob»        → финал «@bob проверь, пожалуйста (e2e)» — будит агента bob;
//   иначе               → финал «Готово (e2e mock): <текст без @>».
//
// В эхо «@» убирается: ответ агента не должен упоминать людей и будить их
// агентов по кругу. Запускается контейнером node:22 (см. e2e/run.sh).
const http = require('http');

const QUESTION = 'Разрешить деплой на прод?';

const SHELL = {
  'создай файл': 'echo e2e-content > e2e-created.txt',
  'запусти curl': 'curl -s http://example.com',
  'запиши вне папки': 'echo e2e > ../outside.txt',
};

const text = (m) => (typeof m?.content === 'string' ? m.content : JSON.stringify(m?.content ?? ''));
const final = (content) => ({ role: 'assistant', content });
const call = (name, args) => ({
  role: 'assistant',
  content: null,
  tool_calls: [{ id: `c${Date.now()}`, type: 'function', function: { name, arguments: JSON.stringify(args) } }],
});

function answer(body) {
  const messages = body.messages || [];
  const last = messages[messages.length - 1];
  if (last && last.role === 'tool') return final(`Готово (e2e mock): ${text(last).trim()}`);

  const users = messages.filter((m) => m.role === 'user');
  const said = text(users[users.length - 1]);
  if (said.includes('выкати прод')) return call('ask_human', { question: QUESTION });
  for (const [trigger, command] of Object.entries(SHELL)) {
    if (said.includes(trigger)) return call('shell', { command });
  }
  if (said.includes('спроси bob')) return final('@bob проверь, пожалуйста (e2e)');
  return final(`Готово (e2e mock): ${said.replaceAll('@', '').slice(-200)}`);
}

http.createServer((req, res) => {
  let raw = '';
  req.on('data', (chunk) => (raw += chunk));
  req.on('end', () => {
    let body = {};
    try { body = JSON.parse(raw); } catch {}
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ choices: [{ message: answer(body) }] }));
  });
}).listen(8000);

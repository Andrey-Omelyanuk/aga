// Детерминированная mock-LLM (OpenAI-compatible) для e2e: ответ зависит только
// от последнего сообщения пользователя в запросе.
//   - «выкати прод» → нативный вызов ask_human с вопросом QUESTION;
//   - иначе → финал «Готово (e2e mock): <последнее сообщение>».
// Запускается контейнером node:22 в сети агент-рантайма (см. e2e/run.sh).
const http = require('http');

const QUESTION = 'Разрешить деплой на прод?';

function lastUserText(body) {
  const user = (body.messages || []).filter((m) => m.role === 'user');
  const last = user[user.length - 1];
  const content = last ? last.content : '';
  return typeof content === 'string' ? content : JSON.stringify(content);
}

http.createServer((req, res) => {
  let raw = '';
  req.on('data', (chunk) => (raw += chunk));
  req.on('end', () => {
    let body = {};
    try { body = JSON.parse(raw); } catch {}
    const text = lastUserText(body);
    const message = text.includes('выкати прод')
      ? { role: 'assistant', content: null, tool_calls: [{ id: 'q1', type: 'function',
          function: { name: 'ask_human', arguments: JSON.stringify({ question: QUESTION }) } }] }
      : { role: 'assistant', content: `Готово (e2e mock): ${text.slice(-200)}` };
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ choices: [{ message }] }));
  });
}).listen(8000);

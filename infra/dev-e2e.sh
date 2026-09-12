#!/usr/bin/env bash
# E2E рабочий цикл агента на проекте mobx-model-ui (dev-стенд).
#
# Проверяет вертикальный срез на живом стенде: HTTP API через SSO (роли
# участника и суперпользователя), жизненный цикл воркстейшна (закрыть сессию,
# отпустить, открыть сессию с проектом mobx-model-ui — ядро разворачивает
# git-клон в /work/project), сессию и агент-рантайм (aga agent): сообщение
# alice — привязанного пользователя агента ui — уходит в Centrifugo, рантайм
# слушает её канал и отвечает от её имени через маленькую LLM dev-стенда.
# Качество ответа не проверяем — хватает непустого ответа с артефактом.
#
# Вторая секция — human-in-the-loop ([ASK_HUMAN]) с детерминированной
# mock-LLM (контейнер node, не ollama): вопрос агента появляется в чате текстом
# (не «Request ID»), ответ — сообщение несвязанного участника (bob) с parent_id
# на сообщение-вопрос; он закрывает запрос и возобновляет агента (mock отвечает
# финалом). Так проверяется вся цепочка: каналы чата в подписке рантайма,
# route_answer, статусы задач, продолжение от имени связанного пользователя.
#
# Требует поднятого dev-стенда (`make dev-up`), jq и SSH-доступа по
# AGA_SSH_PRIVATE_KEY к git@github.com:Andrey-Omelyanuk/mobx-model-ui.git.
set -euo pipefail

CORE="${CORE:-http://localhost:${PORT:-8080}}"
KC="${KC:-http://localhost:${KEYCLOAK_PORT:-8082}}"
MOBX_GIT_URL="git@github.com:Andrey-Omelyanuk/mobx-model-ui.git"

echo "==> wait for core API"
for _ in $(seq 1 90); do
  code=$(curl -s -o /dev/null -w '%{http_code}' "$CORE/users" 2>/dev/null || true)
  [ "$code" = "401" ] || [ "$code" = "200" ] && break
  sleep 2
done
[ "$(curl -s -o /dev/null -w '%{http_code}' "$CORE/users")" = "401" ] || \
  [ "$(curl -s -o /dev/null -w '%{http_code}' "$CORE/users")" = "200" ]

# Сбрасываем БД в детерминированное состояние: секция ASK_HUMAN подменяет состав
# набора — повторный прогон скрипта (без `make dev-e2e`, который сеет сам) должен
# стартовать с чистых фикстур.
docker exec aga-core /app/aga seed >/dev/null

# Перезапускаем агент-рантайм: после сида он мог висеть на каналах до привязок;
# свежий старт перечитывает listen_user_id из БД и подписывается заново.
docker restart aga-agent >/dev/null 2>&1 || true

echo "==> wait for Keycloak realm"
for _ in $(seq 1 90); do
  curl -sf "$KC/realms/aga" >/dev/null 2>&1 && break
  sleep 2
done
curl -sf "$KC/realms/aga" | jq -e '.realm == "aga"' >/dev/null

get_token() {
  local user="$1" pass="$2"
  curl -sf -X POST "$KC/realms/aga/protocol/openid-connect/token" \
    -d grant_type=password -d client_id=aga -d client_secret=aga-secret \
    -d "username=$user" -d "password=$pass" | jq -r '.access_token'
}

ALICE=$(get_token alice alice-pass)
BOB=$(get_token bob bob-pass)
[ -n "$ALICE" ] && [ -n "$BOB" ]

echo "==> seed project mobx-model-ui exists"
PROJECT_ID=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/projects" \
  | jq -r --arg u "$MOBX_GIT_URL" '.[] | select(.git_url == $u) | .id' | head -1)
[ -n "$PROJECT_ID" ]
echo "mobx-model-ui project id=$PROJECT_ID"

echo "==> find a workstation with an open session (seed: ws-1)"
WS_ID=""
for id in $(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/workstations" \
  | jq -r '.[].id'); do
  if [ "$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/workstations/$id/session" \
    | jq -r '.id // empty')" != "" ]; then
    WS_ID=$id
    break
  fi
done
[ -n "$WS_ID" ]
echo "workstation id=$WS_ID"

echo "==> wait for the workstation container ready (entrypoint: user aga owns /work)"
WS_READY=""
for _ in $(seq 1 120); do
  if [ "$(docker exec ws-$WS_ID sh -c 'stat -c %u /work' 2>/dev/null || true)" = "1000" ]; then
    WS_READY=1
    break
  fi
  sleep 2
done
[ -n "$WS_READY" ]

echo "==> close its session as the owner (alice) frees the workstation"
SESSION_ID=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/workstations/$WS_ID/session" | jq -r '.id')
[ -n "$SESSION_ID" ]
curl -sf -X POST -H "Authorization: Bearer $ALICE" "$CORE/chats/$SESSION_ID/close" >/dev/null
[ "$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/workstations/$WS_ID/session" | jq -r '.id // empty')" = "" ]
echo "session $SESSION_ID closed, workstation free"

echo "==> release the workstation (bob, superuser)"
curl -sf -X POST -H "Authorization: Bearer $BOB" "$CORE/workstations/$WS_ID/release" >/dev/null
[ "$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/workstations" \
  | jq -r --argjson id "$WS_ID" '.[] | select(.id == $id) | .project_id')" = "0" ]

echo "==> open a session on the workstation (the session deploys the project)"
CHAT_ID=$(curl -sf -X POST -H "Authorization: Bearer $ALICE" -H 'content-type: application/json' \
  "$CORE/workstations/$WS_ID/session" \
  -d "{\"project_id\": $PROJECT_ID, \"title\":\"e2e: mobx-model-ui\"}" | jq -r '.id')
[ -n "$CHAT_ID" ]
echo "session chat id=$CHAT_ID"

echo "==> project code appears in /work/project of the workstation"
CODE_OK=""
for _ in $(seq 1 60); do
  if curl -sf -H "Authorization: Bearer $ALICE" "$CORE/workstations/$WS_ID/tree" \
    | jq -e --arg n README.md '[.entries[].name] | index($n)' >/dev/null 2>&1; then
    CODE_OK=1
    break
  fi
  sleep 2
done
[ -n "$CODE_OK" ]
echo "mobx-model-ui code cloned (README.md present)"

echo "==> ask the agent what the project is (agent ui listens to alice)"
QRES=$(curl -sf -X POST -H "Authorization: Bearer $ALICE" -H 'content-type: application/json' \
  "$CORE/chats/$CHAT_ID/messages" \
  -d '{"body":"что за проект?"}')
QID=$(echo "$QRES" | jq -r '.message.id')
[ -n "$QID" ] && [ "$QID" != "null" ]

ALICE_ID=$(echo "$QRES" | jq -r '.message.author_id')
[ -n "$ALICE_ID" ]

echo "==> wait for a non-empty agent reply as alice (origin=agent) with an artifact"
REPLY_OK=""
for _ in $(seq 1 300); do
  MSG=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/chats/$CHAT_ID/messages" \
    | jq -c --argjson a "$ALICE_ID" --argjson q "$QID" \
      '[.[] | select(.origin == "agent" and .author_id == $a and .id > $q and (.body | length > 0))] | last // empty')
  if [ -n "$MSG" ]; then
    BODY=$(echo "$MSG" | jq -r '.body')
    case "$BODY" in
      "Ошибка:"*) echo "FAIL: agent reply is an error: $BODY" >&2; exit 1 ;;
    esac
    MID=$(echo "$MSG" | jq -r '.id')
    if [ "$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/messages/$MID/artifacts" | jq 'length')" -gt 0 ]; then
      REPLY_OK=1
      break
    fi
  fi
  sleep 2
done
[ -n "$REPLY_OK" ]
echo "agent replied as alice (message $MID, origin=agent), artifact attached"
echo "reply: $BODY"

# === ASK_HUMAN: вопрос в чат, ответ по parent_id от несвязанного участника ===
# Детерминированная mock-LLM вместо ollama: первый запрос к LLM — [ASK_HUMAN],
# последующие — финальный ответ. Так проверяется вся цепочка human-in-the-loop:
# вопрос публикуется текстом (не «Request ID»), задача ждёт, ответ с parent_id
# закрывает запрос и возобновляет агента — даже от участника без привязки (bob).
echo "==> mock LLM for ASK_HUMAN"
AGENT_NET=$(docker inspect aga-agent \
  --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}')
docker rm -f aga-llm-mock >/dev/null 2>&1 || true
trap 'docker rm -f aga-llm-mock >/dev/null 2>&1 || true' EXIT
cat > /tmp/aga-e2e-llm.js <<'EOF'
const http = require('http');
let calls = 0;
http.createServer((req, res) => {
  req.resume();
  req.on('end', () => {
    calls += 1;
    const content = calls === 1
      ? '[ASK_HUMAN] Разрешить деплой на прод?[/ASK_HUMAN]'
      : 'Деплой выполнен (e2e).';
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ choices: [{ message: { role: 'assistant', content } }] }));
  });
}).listen(8000);
EOF
docker run -d --name aga-llm-mock --network "$AGENT_NET" \
  -v /tmp/aga-e2e-llm.js:/s.js:ro node:22 node /s.js >/dev/null

MOCK_LLM=$(curl -sf -X POST -H "Authorization: Bearer $BOB" -H 'content-type: application/json' \
  "$CORE/llms" \
  -d '{"name":"e2e-mock","api_url":"http://aga-llm-mock:8000/v1","model_name":"mock"}' \
  | jq -r '.id')
[ -n "$MOCK_LLM" ] && [ "$MOCK_LLM" != "null" ]

# Набор из одного агента на mock-LLM, привязанного к alice, — проект переключается
# на него (состав резолвится на каждый запуск, рестарт рантайма не нужен).
jq -n --argjson llm "$MOCK_LLM" --argjson alice "$ALICE_ID" '{
  name: "e2e-ask-human",
  agents: [{ name: "echo", description: "e2e-агент на mock-LLM", tools: [],
             max_iterations: 2, llm_id: $llm, parent: null, skills: [], commands: [],
             listen_user_id: $alice }],
}' > /tmp/aga-e2e-set.json
ASK_SET=$(curl -sf -X POST -H "Authorization: Bearer $BOB" -H 'content-type: application/json' \
  "$CORE/agent-sets" --data @/tmp/aga-e2e-set.json | jq -r '.id')
[ -n "$ASK_SET" ] && [ "$ASK_SET" != "null" ]
curl -sf -X POST -H "Authorization: Bearer $BOB" -H 'content-type: application/json' \
  "$CORE/projects/$PROJECT_ID/agent-set" -d "{\"agent_set_id\": $ASK_SET}" >/dev/null

# Рестарт рантайма: перечитать подписку на канал свежей сессии (ответ bob придёт
# по каналу чата, а не по его личному — bob ни к одному агенту не привязан).
docker restart aga-agent >/dev/null 2>&1 || true

echo "==> agent question appears in chat as text (not a Request ID)"
TID=$(curl -sf -X POST -H "Authorization: Bearer $ALICE" -H 'content-type: application/json' \
  "$CORE/chats/$CHAT_ID/messages" -d '{"body":"e2e: выкати прод"}' | jq -r '.message.id')
QID=""
for _ in $(seq 1 150); do
  Q=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/chats/$CHAT_ID/messages" \
    | jq -c --argjson t "$TID" \
      '[.[] | select(.origin == "agent" and .id > $t and (.body | contains("Разрешить деплой")))] | last // empty')
  if [ -n "$Q" ]; then
    QID=$(echo "$Q" | jq -r '.id')
    break
  fi
  sleep 2
done
[ -n "$QID" ]
# Сырых служебных строк в чате быть не должно.
if curl -sf -H "Authorization: Bearer $ALICE" "$CORE/chats/$CHAT_ID/messages" \
  | jq -e '[.[] | select(.body | contains("Request ID") or contains("WAITING_FOR_HUMAN"))] | length > 0' \
  >/dev/null; then
  echo "FAIL: service string leaked into chat" >&2; exit 1
fi
echo "question in chat: message $QID"

echo "==> bob (unbound participant) answers with parent_id — agent resumes"
AID=$(curl -sf -X POST -H "Authorization: Bearer $BOB" -H 'content-type: application/json' \
  "$CORE/chats/$CHAT_ID/messages" -d "{\"body\":\"да\",\"parent_id\": $QID}" \
  | jq -r '.message.id')
FINAL=""
for _ in $(seq 1 150); do
  F=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/chats/$CHAT_ID/messages" \
    | jq -c --argjson a "$AID" --argjson alice "$ALICE_ID" \
      '[.[] | select(.origin == "agent" and .author_id == $alice and .id > $a
                    and (.body | contains("Деплой выполнен")))] | last // empty')
  if [ -n "$F" ]; then
    FINAL=$(echo "$F" | jq -r '.id')
    break
  fi
  sleep 2
done
[ -n "$FINAL" ]
echo "resume done: final agent reply (message $FINAL) after bob's answer"
docker rm -f aga-llm-mock >/dev/null 2>&1 || true
trap - EXIT

echo "==> OK"
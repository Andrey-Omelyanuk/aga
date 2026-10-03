#!/usr/bin/env bash
# E2E dev-стенда: сквозные сценарии, которые юнит-тесты проверить не могут —
# настоящие Keycloak, Centrifugo, docker-воркстейшны, git по SSH, отдельный
# процесс агент-рантайма и LLM по HTTP. Что и почему проверяется — infra/E2E.md.
#
#   1. SSO: аноним получает 401, токены Keycloak принимаются, роль admin (bob)
#      даёт права суперпользователя.
#   2. Воркстейшн: владелец закрывает сессию, админ отпускает станцию, новая
#      сессия с проектом разворачивает git-клон в /work/project.
#   3. Агент отвечает: @alice в чате → Centrifugo → `aga agent` → LLM (ollama)
#      → ответ от имени alice (origin=agent) с артефактом. Качество ответа не
#      проверяем — маленькая LLM недетерминирована.
#   4. ask_human: детерминированная mock-LLM задаёт вопрос, он виден в чате
#      текстом; ответ bob с parent_id возобновляет агента до финала.
#
# Требует поднятого dev-стенда (`make dev-up`; `make dev-e2e` пересобирает
# ядро/рантайм/воркстейшны и запускает скрипт), jq и SSH-доступа по
# AGA_SSH_PRIVATE_KEY к git@github.com:Andrey-Omelyanuk/mobx-model-ui.git.
set -euo pipefail

CORE="${CORE:-http://localhost:${PORT:-8080}}"
KC="${KC:-http://localhost:${KEYCLOAK_PORT:-8082}}"
MOBX_GIT_URL="git@github.com:Andrey-Omelyanuk/mobx-model-ui.git"
TMP=$(mktemp -d)
cleanup() {
  docker rm -f aga-llm-mock >/dev/null 2>&1 || true
  rm -rf "$TMP"
}
trap cleanup EXIT

step() { echo "==> $*"; }
fail() { echo "FAIL: $*" >&2; exit 1; }

# Токен берём на каждый запрос: access-токен Keycloak живёт 5 минут, а ожидание
# ответа маленькой LLM бывает дольше.
token() {
  curl -sf -X POST "$KC/realms/aga/protocol/openid-connect/token" \
    -d grant_type=password -d client_id=aga -d client_secret=aga-secret \
    -d "username=$1" -d "password=$1-pass" | jq -r '.access_token'
}

# api <user> <METHOD> <path> [json-body]
api() {
  local user="$1" method="$2" path="$3" body="${4:-}"
  local args=(-sf -X "$method" -H "Authorization: Bearer $(token "$user")")
  [ -n "$body" ] && args+=(-H 'content-type: application/json' -d "$body")
  curl "${args[@]}" "$CORE$path"
}

# wait_for <секунды> <команда...> — повторяет команду раз в 2 с, пока она не
# напечатает непустую строку; печатает её. Пусто по таймауту — ошибка.
wait_for() {
  local timeout="$1"; shift
  local out
  for _ in $(seq 1 $((timeout / 2))); do
    out=$("$@" 2>/dev/null || true)
    if [ -n "$out" ]; then echo "$out"; return 0; fi
    sleep 2
  done
  return 1
}

# Последнее сообщение агента от имени alice после сообщения <after_id>, тело
# которого содержит <text> (пусто — любое непустое).
agent_reply() {
  local chat="$1" after="$2" text="${3:-}"
  api alice GET "/chats/$chat/messages" | jq -c \
    --argjson a "$ALICE_ID" --argjson after "$after" --arg t "$text" \
    '[.[] | select(.origin == "agent" and .author_id == $a and .id > $after
                  and (.body | length > 0) and (.body | contains($t)))] | last // empty'
}

# --- подготовка: детерминированная БД и свежий рантайм ----------------------
step "wait for core API and Keycloak realm"
wait_for 180 sh -c "curl -s -o /dev/null -w '%{http_code}' '$CORE/users' | grep -x 401" >/dev/null \
  || fail "core API is not up"
wait_for 180 sh -c "curl -sf '$KC/realms/aga' | jq -e 'select(.realm == \"aga\")'" >/dev/null \
  || fail "Keycloak realm aga is not up"

# Сид сбрасывает БД (сценарий 4 подменяет набор — повторный прогон стартует с
# чистых фикстур), рестарт рантайма — свежие привязки агентов после сида.
docker exec aga-core /app/aga seed >/dev/null
docker restart aga-agent >/dev/null

# --- 1. SSO -----------------------------------------------------------------
step "1. SSO: anonymous gets 401, Keycloak tokens are accepted"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$CORE/users")" = "401" ] || fail "anonymous not rejected"
ALICE_ID=$(api alice GET /users/me | jq -r '.id')
[ -n "$ALICE_ID" ] && [ "$ALICE_ID" != "null" ] || fail "alice token not accepted"
api bob GET /users/me >/dev/null || fail "bob token not accepted"
echo "alice id=$ALICE_ID"

# --- 2. Жизненный цикл воркстейшна -------------------------------------------
step "2. workstation: seed project and ws with an open session exist"
PROJECT_ID=$(api alice GET /projects \
  | jq -r --arg u "$MOBX_GIT_URL" '[.[] | select(.git_url == $u)][0].id // empty')
[ -n "$PROJECT_ID" ] || fail "seed project mobx-model-ui not found"
WS_ID=""
for id in $(api alice GET /workstations | jq -r '.[].id'); do
  if [ -n "$(api alice GET "/workstations/$id/session" | jq -r '.id // empty')" ]; then
    WS_ID=$id; break
  fi
done
[ -n "$WS_ID" ] || fail "no workstation with an open session (seed: ws-1)"
echo "project id=$PROJECT_ID, workstation id=$WS_ID"
# entrypoint станции отдаёт /work пользователю aga (uid 1000) — признак готовности.
wait_for 240 sh -c "docker exec ws-$WS_ID stat -c %u /work | grep -x 1000" >/dev/null \
  || fail "workstation container ws-$WS_ID not ready"

step "2. workstation: owner (alice) closes the session, admin (bob) releases the station"
SESSION_ID=$(api alice GET "/workstations/$WS_ID/session" | jq -r '.id')
api alice POST "/chats/$SESSION_ID/close" >/dev/null
[ -z "$(api alice GET "/workstations/$WS_ID/session" | jq -r '.id // empty')" ] \
  || fail "session $SESSION_ID still open"
api bob POST "/workstations/$WS_ID/release" >/dev/null || fail "admin release rejected"
[ "$(api alice GET /workstations | jq -r --argjson id "$WS_ID" '.[] | select(.id == $id) | .project_id')" = "0" ] \
  || fail "workstation still bound to a project"

step "2. workstation: a new session deploys the project (git clone over SSH)"
CHAT_ID=$(api alice POST "/workstations/$WS_ID/session" \
  "{\"project_id\": $PROJECT_ID, \"title\": \"e2e: mobx-model-ui\"}" | jq -r '.id')
[ -n "$CHAT_ID" ] && [ "$CHAT_ID" != "null" ] || fail "session not opened"
readme_cloned() {
  api alice GET "/workstations/$WS_ID/tree" | jq -e '.entries[] | select(.name == "README.md")'
}
wait_for 120 readme_cloned >/dev/null || fail "project code did not appear in /work/project"
echo "session chat id=$CHAT_ID, README.md cloned"

# --- 3. Агент отвечает через живую LLM ---------------------------------------
step "3. agent: bob asks @alice, the agent (ui) answers as alice via ollama"
QID=$(api bob POST "/chats/$CHAT_ID/messages" '{"body":"@alice что за проект?"}' | jq -r '.message.id')
MSG=$(wait_for 600 agent_reply "$CHAT_ID" "$QID") || fail "no agent reply in 10 min"
BODY=$(echo "$MSG" | jq -r '.body')
MID=$(echo "$MSG" | jq -r '.id')
case "$BODY" in "Ошибка:"*) fail "agent reply is an error: $BODY" ;; esac
[ "$(api alice GET "/messages/$MID/artifacts" | jq 'length')" -gt 0 ] || fail "reply has no artifact"
echo "reply (message $MID): $BODY"

# --- 4. ask_human на mock-LLM -------------------------------------------------
# Mock: первый запрос — нативный вызов ask_human, дальше — финальный ответ.
step "4. ask_human: switch the project to a one-agent set on a mock LLM"
cat > "$TMP/llm.js" <<'EOF'
const http = require('http');
let calls = 0;
http.createServer((req, res) => {
  req.resume();
  req.on('end', () => {
    calls += 1;
    const message = calls === 1
      ? { role: 'assistant', content: null, tool_calls: [{ id: 'q1', type: 'function',
          function: { name: 'ask_human',
            arguments: JSON.stringify({ question: 'Разрешить деплой на прод?' }) } }] }
      : { role: 'assistant', content: 'Деплой выполнен (e2e).' };
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ choices: [{ message }] }));
  });
}).listen(8000);
EOF
AGENT_NET=$(docker inspect aga-agent --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}')
docker rm -f aga-llm-mock >/dev/null 2>&1 || true
docker run -d --name aga-llm-mock --network "$AGENT_NET" \
  -v "$TMP/llm.js":/s.js:ro node:22 node /s.js >/dev/null

MOCK_LLM=$(api bob POST /llms \
  '{"name":"e2e-mock","api_url":"http://aga-llm-mock:8000/v1","model_name":"mock"}' | jq -r '.id')
SET=$(jq -n --argjson llm "$MOCK_LLM" --argjson alice "$ALICE_ID" '{
  name: "e2e-ask-human",
  agents: [{ name: "echo", description: "e2e-агент на mock-LLM", tools: [],
             max_iterations: 2, llm_id: $llm, parent: null, skills: [],
             listen_user_id: $alice }]}')
SET_ID=$(api bob POST /agent-sets "$SET" | jq -r '.id')
api bob POST "/projects/$PROJECT_ID/agent-set" "{\"agent_set_id\": $SET_ID}" >/dev/null
# Рантайм перечитывает привязки при переподключении — рестарт делает это сразу.
docker restart aga-agent >/dev/null

step "4. ask_human: the agent's question appears in chat as plain text"
TID=$(api bob POST "/chats/$CHAT_ID/messages" '{"body":"@alice e2e: выкати прод"}' | jq -r '.message.id')
Q=$(wait_for 300 agent_reply "$CHAT_ID" "$TID" "Разрешить деплой") || fail "question not posted"
QID=$(echo "$Q" | jq -r '.id')
api alice GET "/chats/$CHAT_ID/messages" \
  | jq -e '[.[] | select(.body | contains("Request ID") or contains("WAITING_FOR_HUMAN"))] | length == 0' \
  >/dev/null || fail "service string leaked into chat"
echo "question: message $QID"

step "4. ask_human: bob (not bound to any agent) answers with parent_id — the agent resumes"
AID=$(api bob POST "/chats/$CHAT_ID/messages" "{\"body\":\"да\",\"parent_id\": $QID}" | jq -r '.message.id')
F=$(wait_for 300 agent_reply "$CHAT_ID" "$AID" "Деплой выполнен") || fail "agent did not resume"
echo "final reply: message $(echo "$F" | jq -r '.id')"

echo "==> OK"

#!/usr/bin/env bash
# E2E рабочий цикл агента на проекте mobx-model-ui (dev-стенд).
#
# Проверяет вертикальный срез на живом стенде: HTTP API через SSO (роли
# участника и суперпользователя), жизненный цикл воркстейшна (закрыть сессию,
# отпустить, переключить на mobx-model-ui — git-клон в /work/project), сессию
# и реактивного агента (@Agent.ui), который отвечает о проекте через маленькую
# LLM dev-стенда. Качество ответа не проверяем — хватает непустого ответа с
# артефактом.
#
# Требует поднятого и засеянного dev-стенда (`make dev-up`, `make dev-seed`),
# jq и SSH-доступа по AGA_SSH_PRIVATE_KEY к git@github.com:Andrey-Omelyanuk/mobx-model-ui.git.
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

echo "==> switch released workstation to mobx-model-ui (bob, superuser)"
curl -sf -X POST -H "Authorization: Bearer $BOB" -H 'content-type: application/json' \
  "$CORE/workstations/$WS_ID/switch" -d "{\"project_id\": $PROJECT_ID}" >/dev/null

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

echo "==> open a session on the workstation"
CHAT_ID=$(curl -sf -X POST -H "Authorization: Bearer $ALICE" -H 'content-type: application/json' \
  "$CORE/workstations/$WS_ID/session" -d '{"title":"e2e: mobx-model-ui"}' | jq -r '.id')
[ -n "$CHAT_ID" ]
echo "session chat id=$CHAT_ID"

echo "==> ask the agent what the project is (@Agent.ui)"
curl -sf -X POST -H "Authorization: Bearer $ALICE" -H 'content-type: application/json' \
  "$CORE/chats/$CHAT_ID/messages" \
  -d '{"body":"@Agent.ui что за проект?"}' >/dev/null

AGENT_ID=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/users" \
  | jq -r '.[] | select(.name == "Agent.ui") | .id' | head -1)
[ -n "$AGENT_ID" ]

echo "==> wait for a non-empty reply from the agent with an artifact"
REPLY_OK=""
for _ in $(seq 1 300); do
  MSG=$(curl -sf -H "Authorization: Bearer $ALICE" "$CORE/chats/$CHAT_ID/messages" \
    | jq -c --argjson a "$AGENT_ID" '[.[] | select(.author_id == $a and (.body | length > 0))] | last // empty')
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
echo "agent replied (message $MID), artifact attached"
echo "reply: $BODY"

echo "==> OK"
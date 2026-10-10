#!/usr/bin/env bash
# Запуск e2e (Playwright) против dev-стенда. Готовит стенд — детерминированная
# БД (сид), свежий агент-рантайм, mock-LLM для сценария ask_human — и гоняет
# браузерные тесты в официальном образе Playwright (на хосте ставить ничего не
# нужно). Аргументы передаются в `playwright test` (например, --headed не
# работает в контейнере, а `-g "ask_human"` — да). Описание — e2e/README.md.
set -euo pipefail
cd "$(dirname "$0")/.."

# Инстанс dev-стенда = Linux-пользователь: контейнеры с префиксом, свой проект
# compose, свои порты. Переменные экспортирует makefile (make dev-e2e); при
# ручном запуске выводим те же значения из окружения/$USER.
INSTANCE="${AGA_INSTANCE:-${INSTANCE:-$USER}}"
NAME_PREFIX="${NAME_PREFIX:-aga-${INSTANCE}}"
MOCK_NAME="${NAME_PREFIX}-llm-mock"
PROXY_PORT="${AGA_PROXY_PORT:-8080}"
KEYCLOAK_PORT="${KEYCLOAK_PORT:-8082}"
CORE="${CORE:-http://localhost:${AGA_CORE_PORT:-8080}}"
E2E_BASE_URL="${E2E_BASE_URL:-http://dev.${INSTANCE}.localhost:${PROXY_PORT}}"
DC=(docker compose --env-file .env -p "aga-${INSTANCE}")

PLAYWRIGHT_IMAGE="mcr.microsoft.com/playwright:v1.63.0-noble"

cleanup() { docker rm -f "$MOCK_NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT

echo "==> stand is up ($INSTANCE, base $E2E_BASE_URL)"
for _ in $(seq 1 90); do
  [ "$(curl -s -o /dev/null -w '%{http_code}' "$CORE/users" || true)" = "401" ] && break
  sleep 2
done
[ "$(curl -s -o /dev/null -w '%{http_code}' "$CORE/users")" = "401" ] \
  || { echo "FAIL: core API is not up — run 'make dev-up'" >&2; exit 1; }

echo "==> seed DB, restart agent runtime, reload proxy"
"${DC[@]}" exec -T core /app/aga seed >/dev/null
"${DC[@]}" restart agent >/dev/null
# Прокси держит IP пересозданных контейнеров — без reload api.<instance> отдаёт 502.
"${DC[@]}" exec -T proxy nginx -s reload >/dev/null 2>&1

echo "==> mock LLM (e2e/fixtures/mock-llm.js)"
AGENT_NET=$(docker inspect "${NAME_PREFIX}-agent" --format '{{range $k, $v := .NetworkSettings.Networks}}{{$k}}{{end}}')
cleanup
docker run -d --name "$MOCK_NAME" --network "$AGENT_NET" \
  -v "$PWD/e2e/fixtures/mock-llm.js":/s.js:ro node:22 node /s.js >/dev/null

# Тесты устойчивости перезапускают контейнеры стенда: в контейнер Playwright
# пробрасываем docker-сокет и статический docker CLI (из образа docker:27-cli).
if [ ! -x e2e/.bin/docker ]; then
  mkdir -p e2e/.bin
  CLI=$(docker create docker:27-cli)
  docker cp "$CLI":/usr/local/bin/docker e2e/.bin/docker >/dev/null
  docker rm "$CLI" >/dev/null
fi

echo "==> playwright"
rm -rf e2e/.auth
docker run --rm --network host --ipc host \
  --user "$(id -u):$(id -g)" --group-add "$(stat -c %g /var/run/docker.sock)" \
  -e HOME=/tmp -e CI=1 -e PATH="/e2e/.bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
  -e E2E_LLM_MODEL="${E2E_LLM_MODEL:-}" \
  -e E2E_BASE_URL="$E2E_BASE_URL" \
  -e E2E_CORE="$CORE" \
  -e E2E_KEYCLOAK="http://localhost:${KEYCLOAK_PORT}" \
  -e E2E_CONTAINER_PREFIX="$NAME_PREFIX" \
  -e E2E_MOCK_LLM_URL="http://${MOCK_NAME}:8000/v1" \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v "$PWD/e2e":/e2e -w /e2e "$PLAYWRIGHT_IMAGE" \
  sh -c '[ -d node_modules/@playwright/test ] || npm ci --no-audit --no-fund --loglevel=error; exec npx playwright test "$@"' \
  playwright "$@"

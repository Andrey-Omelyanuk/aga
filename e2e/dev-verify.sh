#!/usr/bin/env bash
# Быстрая проверка, что dev-стенд поднят: Keycloak, ядро с SSO, воркстейшны,
# маленькая LLM, фронт и прокси *.localhost. Сценариев здесь нет — они в
# e2e/dev.sh. Подробно — e2e/README.md.
set -euo pipefail
cd "$(dirname "$0")/.."

# Инстанс = Linux-пользователь (переменные экспортирует makefile).
INSTANCE="${AGA_INSTANCE:-${INSTANCE:-$USER}}"
NAME_PREFIX="${NAME_PREFIX:-aga-${INSTANCE}}"
WS_PREFIX="${WS_PREFIX:-${NAME_PREFIX}-}"
PROXY_PORT="${AGA_PROXY_PORT:-8080}"
CORE="${CORE:-http://localhost:${AGA_CORE_PORT:-8080}}"
FRONT="${FRONT:-http://localhost:${AGA_FRONT_PORT:-8081}}"
LOCAL=(--resolve "dev.${INSTANCE}.localhost:${PROXY_PORT}:127.0.0.1"
       --resolve "api.${INSTANCE}.localhost:${PROXY_PORT}:127.0.0.1"
       --resolve "auth.${INSTANCE}.localhost:${PROXY_PORT}:127.0.0.1")

ok() { echo "$* OK"; }
fail() { echo "FAIL: $*" >&2; exit 1; }
code() { curl -s -o /dev/null -w '%{http_code}' "${LOCAL[@]}" "$@"; }

# wait_for <секунды> <команда...> — повторяет команду раз в 2 с до успеха.
wait_for() {
  local timeout="$1"; shift
  for _ in $(seq 1 $((timeout / 2))); do
    "$@" >/dev/null 2>&1 && return 0
    sleep 2
  done
  return 1
}

echo "Waiting for Keycloak realm (auth.${INSTANCE}.localhost)..."
wait_for 120 sh -c "curl -s ${LOCAL[*]} http://auth.${INSTANCE}.localhost:${PROXY_PORT}/realms/aga | grep -q '\"realm\"'" \
  || fail "Keycloak realm aga"
# Ядро поднимает HTTP только после JWKS Keycloak — ждём и его.
wait_for 120 sh -c "[ \"\$(curl -s -o /dev/null -w '%{http_code}' $CORE/users)\" = 401 ]" \
  || fail "core API"

[ "$(code "$CORE/users")" = "401" ] && ok "core SSO: anonymous rejected" || fail "core SSO"
[ "$(code "$CORE/auth/login")" = "307" ] && ok "core auth/login redirect" || fail "core auth/login"
for ws in 1 2; do
  docker exec "${WS_PREFIX}ws-${ws}" sh -c "test -d /work/project/.git" \
    && ok "${WS_PREFIX}ws-${ws}" || fail "${WS_PREFIX}ws-${ws}"
done

echo "Waiting for small LLM model (ollama:qwen3:0.6b)..."
wait_for 120 sh -c "docker exec ${NAME_PREFIX}-ollama ollama list | grep -q 'qwen3:0.6b'" || fail "ollama qwen3:0.6b"
ok "ollama small LLM"

curl -fsS "$FRONT/" >/dev/null && ok "front" || fail "front"
curl -s "${LOCAL[@]}" "http://auth.${INSTANCE}.localhost:${PROXY_PORT}/realms/aga" | grep -q '"realm"' \
  && ok "proxy auth.${INSTANCE}.localhost (keycloak)" || fail "proxy auth"
curl -fsS "${LOCAL[@]}" "http://dev.${INSTANCE}.localhost:${PROXY_PORT}/" >/dev/null \
  && ok "proxy dev.${INSTANCE}.localhost" || fail "proxy dev"
[ "$(code "http://api.${INSTANCE}.localhost:${PROXY_PORT}/users")" = "401" ] \
  && ok "proxy api.${INSTANCE}.localhost (SSO)" || fail "proxy api"

#!/usr/bin/env bash
# Быстрая проверка, что dev-стенд поднят: Keycloak, ядро с SSO, воркстейшны,
# маленькая LLM, фронт и прокси *.localhost; порты стенда — только на loopback,
# Keycloak принимает лишь redirect_uri ядра. Сценариев здесь нет — они в
# e2e/run.sh. Подробно — e2e/README.md.
set -euo pipefail
cd "$(dirname "$0")/.."

# Порты опубликованы только на 127.0.0.1 (dev-compose.yml).
CORE="${CORE:-http://127.0.0.1:${PORT:-8080}}"
FRONT="http://127.0.0.1:${AGA_FRONT_PORT:-8081}"
PROXY_PORT="${AGA_PROXY_PORT:-80}"
# В адресе порт 80 браузер опускает — так же строит адреса стенд.
URL_PORT="$([ "$PROXY_PORT" = 80 ] || echo ":$PROXY_PORT")"
LOCAL=(--resolve "dev.localhost:$PROXY_PORT:127.0.0.1" --resolve "api.localhost:$PROXY_PORT:127.0.0.1"
       --resolve "auth.localhost:$PROXY_PORT:127.0.0.1")
DEV="http://dev.localhost$URL_PORT"
API="http://api.localhost$URL_PORT"
AUTH="http://auth.localhost$URL_PORT"

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

echo "Waiting for Keycloak realm (auth.localhost)..."
wait_for 120 sh -c "curl -s ${LOCAL[*]} $AUTH/realms/aga | grep -q '\"realm\"'" \
  || fail "Keycloak realm aga"
# Ядро поднимает HTTP только после JWKS Keycloak — ждём и его.
wait_for 120 sh -c "[ \"\$(curl -s -o /dev/null -w '%{http_code}' $CORE/users)\" = 401 ]" \
  || fail "core API"

[ "$(code "$CORE/users")" = "401" ] && ok "core SSO: anonymous rejected" || fail "core SSO"
[ "$(code "$CORE/auth/login")" = "307" ] && ok "core auth/login redirect" || fail "core auth/login"
for ws in ws-1 ws-2; do
  docker exec "$ws" sh -c "test -d /work/project/.git" && ok "$ws" || fail "$ws"
done

echo "Waiting for small LLM model (ollama:qwen3:0.6b)..."
wait_for 120 sh -c "docker exec aga-ollama ollama list | grep -q 'qwen3:0.6b'" || fail "ollama qwen3:0.6b"
ok "ollama small LLM"

curl -fsS "$FRONT/" >/dev/null && ok "front" || fail "front"
curl -s "${LOCAL[@]}" "$AUTH/realms/aga" | grep -q '"realm"' \
  && ok "proxy auth.localhost (keycloak)" || fail "proxy auth.localhost"
curl -fsS "${LOCAL[@]}" "$DEV/" >/dev/null && ok "proxy dev.localhost" || fail "proxy dev.localhost"
[ "$(code "$API/users")" = "401" ] && ok "proxy api.localhost (SSO)" || fail "proxy api.localhost"

# Вход через api.localhost: Keycloak принимает redirect_uri ядра
# (<api>/auth/callback из realm) и отвергает чужой.
LOGIN=$(curl -s -o /dev/null -w '%{redirect_url}' "${LOCAL[@]}" "$API/auth/login")
[ "$(code "$LOGIN")" = "200" ] && ok "keycloak accepts core redirect_uri" || fail "keycloak redirect_uri ($LOGIN)"
FOREIGN=$(echo "$LOGIN" | sed 's|redirect_uri=[^&]*|redirect_uri=http%3A%2F%2Fevil.example%2Fauth%2Fcallback|')
[ "$(code "$FOREIGN")" = "400" ] && ok "keycloak rejects foreign redirect_uri" || fail "keycloak accepted foreign redirect_uri"

# Порты стенда не торчат наружу: всё опубликованное — на 127.0.0.1.
EXPOSED=$(docker ps --filter label=com.docker.compose.project=aga-dev --format '{{.Names}} {{.Ports}}' \
  | grep -E '(0\.0\.0\.0|\[::\]|::):[0-9]+->' || true)
[ -z "$EXPOSED" ] && ok "ports bound to loopback only" || fail "ports exposed beyond loopback: $EXPOSED"

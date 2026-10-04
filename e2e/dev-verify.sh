#!/usr/bin/env bash
# Быстрая проверка, что dev-стенд поднят: Keycloak, ядро с SSO, воркстейшны,
# маленькая LLM, фронт и прокси *.localhost. Сценариев здесь нет — они в
# e2e/dev.sh. Подробно — e2e/README.md.
set -euo pipefail
cd "$(dirname "$0")/.."

CORE="${CORE:-http://localhost:${PORT:-8080}}"
FRONT="http://localhost:${AGA_FRONT_PORT:-8081}"
LOCAL=(--resolve dev.localhost:80:127.0.0.1 --resolve api.localhost:80:127.0.0.1
       --resolve auth.localhost:80:127.0.0.1)

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
wait_for 120 sh -c "curl -s ${LOCAL[*]} http://auth.localhost/realms/aga | grep -q '\"realm\"'" \
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
curl -s "${LOCAL[@]}" http://auth.localhost/realms/aga | grep -q '"realm"' \
  && ok "proxy auth.localhost (keycloak)" || fail "proxy auth.localhost"
curl -fsS "${LOCAL[@]}" http://dev.localhost/ >/dev/null && ok "proxy dev.localhost" || fail "proxy dev.localhost"
[ "$(code http://api.localhost/users)" = "401" ] && ok "proxy api.localhost (SSO)" || fail "proxy api.localhost"

#!/usr/bin/env bash
# roles.yaml ядра для стенда (dev-compose — make dev-roles, k8s — core/deploy.sh).
# Блоки до `sso:` берутся из локального main/config/roles.yaml, кроме
# centrifuge: он и sso — стендовые, секреты — из env (.env, их генерирует
# make init). Centrifugo и Keycloak внутри сети стенда — centrifugo:8000 и
# keycloak:8080 (одинаково в compose и k8s). Пишет в stdout.
#
# Usage: bash infra/stand-roles.sh <roles.yaml> <keycloak public url>
#   env: AGA_CENTRIFUGO_API_KEY, AGA_CENTRIFUGO_SECRET, AGA_KC_CLIENT_SECRET
set -euo pipefail

SRC="${1:?usage: stand-roles.sh <roles.yaml> <keycloak public url>}"
KC_PUBLIC="${2:?usage: stand-roles.sh <roles.yaml> <keycloak public url>}"
REALM="aga"

[ -f "$SRC" ] || { echo "roles config not found: $SRC (run 'make init')" >&2; exit 1; }
for v in AGA_CENTRIFUGO_API_KEY AGA_CENTRIFUGO_SECRET AGA_KC_CLIENT_SECRET; do
  [ -n "${!v:-}" ] || { echo "$v is not set — run 'make init' (adds stand secrets to .env)" >&2; exit 1; }
done

# YAML-строка в одинарных кавычках: значение из .env может оказаться числом.
q() { printf "'%s'" "${1//\'/\'\'}"; }

echo "# Сгенерировано infra/stand-roles.sh из $SRC — не править вручную."
# Всё до sso:, без комментариев и centrifuge-блока (он до следующего ключа
# верхнего уровня).
awk '/^sso:/{exit} /^[[:space:]]*#/{next} /^centrifuge:/{skip=1; next} skip && /^[^[:space:]]/{skip=0} !skip{print}' "$SRC"

cat <<EOF
centrifuge:
  api_url: http://centrifugo:8000
  api_key: $(q "$AGA_CENTRIFUGO_API_KEY")
  secret: $(q "$AGA_CENTRIFUGO_SECRET")
  channel: common
sso:
  enabled: true
  jwks_url: http://keycloak:8080/realms/${REALM}/protocol/openid-connect/certs
  authorize_url: ${KC_PUBLIC}/realms/${REALM}/protocol/openid-connect/auth
  token_url: http://keycloak:8080/realms/${REALM}/protocol/openid-connect/token
  end_session_url: ${KC_PUBLIC}/realms/${REALM}/protocol/openid-connect/logout
  client_id: aga
  client_secret: $(q "$AGA_KC_CLIENT_SECRET")
EOF

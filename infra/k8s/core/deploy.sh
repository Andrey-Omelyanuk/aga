#!/usr/bin/env bash
# Поднимает стенд в кластере: namespace aga, RBAC ядра, PVC, конфиги
# (roles + env + realm), Deployment ядра и Keycloak, сервисы. Идемпотентен —
# повторный запуск пересоздаёт только конфиги и приводит манифесты к желаемому.
#
# Конфиги не лежат в git: roles.yaml берётся из main/config/roles.yaml (sso-блок
# заменяется на стендовый), env — из .env, realm — тестовый из этого каталога.
set -euo pipefail

NS="${AGA_K8S_NAMESPACE:-aga}"
KUBECTL="${AGA_K8S_KUBECTL:-kubectl}"
ROLES_SRC="${AGA_ROLES_CONFIG:-./main/config/roles.yaml}"
RUST_LOG="${RUST_LOG:-info}"
PORT="${PORT:-8080}"
# Origin веб-клиента в стенде (CORS ядра + возврат токена после SSO).
AGA_K8S_FRONT_URL="${AGA_K8S_FRONT_URL:-http://dev.localhost}"
REALM="aga"
CLIENT_SECRET="aga-secret"

# Куда браузер пойдёт логиниться. По умолчанию — auth.localhost (через ingress +
# локальный nginx-прокси из make k8s-dev, .localhost резолвится браузером в
# 127.0.0.1). Без ingress переопределить KEYCLOAK_URL, например
# http://<ip>:30081 (NodePort).
KEYCLOAK_URL="${KEYCLOAK_URL:-http://auth.localhost}"

K8S_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ ! -f "$ROLES_SRC" ]; then
  echo "roles config not found: $ROLES_SRC (run 'make init')" >&2
  exit 1
fi

echo "==> namespace"
$KUBECTL apply -f "$K8S_DIR/00-namespace.yaml"

echo "==> configmaps"
# env ядра: порт, лог-уровень. Дефолтной LLM из env нет — подключения к LLM
# живут в БД (создаются на странице «LLM» или сидом).
$KUBECTL create configmap aga-env -n "$NS" \
  --from-literal=AGA_FRONT_URL="$AGA_K8S_FRONT_URL" \
  --from-literal=RUST_LOG="$RUST_LOG" \
  --from-literal=PORT="$PORT" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

# roles.yaml ядра: роли из main/config/roles.yaml, sso-блок — стендовый (включён,
# указывает на Keycloak в кластере). authorize_url — внешний для браузера.
awk '/^sso:/{exit} {print}' "$ROLES_SRC" > "$WORK/roles.yaml"
cat >> "$WORK/roles.yaml" <<EOF
sso:
  enabled: true
  jwks_url: http://keycloak:8080/realms/${REALM}/protocol/openid-connect/certs
  authorize_url: ${KEYCLOAK_URL}/realms/${REALM}/protocol/openid-connect/auth
  token_url: http://keycloak:8080/realms/${REALM}/protocol/openid-connect/token
  end_session_url: ${KEYCLOAK_URL}/realms/${REALM}/protocol/openid-connect/logout
  client_id: aga
  client_secret: ${CLIENT_SECRET}
EOF
$KUBECTL create configmap aga-roles -n "$NS" \
  --from-file=roles.yaml="$WORK/roles.yaml" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

echo "==> ssh secret (env ядра)"
# SSH-ключ aga (git+ssh для воркстейшнов): приватный задаёт админ в
# AGA_SSH_PRIVATE_KEY (из .env); не задан — пустой Secret, ядро работает без
# ключа. Монтируется в под как env (см. 30-deployment-core.yaml envFrom).
$KUBECTL create secret generic aga-ssh-env -n "$NS" \
  --from-literal=AGA_SSH_PRIVATE_KEY="${AGA_SSH_PRIVATE_KEY:-}" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

# realm Keycloak: тестовые участники и клиент aga.
$KUBECTL create configmap aga-keycloak-realm -n "$NS" \
  --from-file=realm.json="$K8S_DIR/keycloak-realm.json" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

echo "==> manifests"
for f in "$K8S_DIR"/*.yaml; do
  $KUBECTL apply -f "$f"
done

echo "==> stand deployed: core + Keycloak in ns '$NS'"
echo "    keycloak:   $KEYCLOAK_URL"
echo "    front:      make k8s-deploy (infra/k8s/front) + make k8s-web"
echo "    wait:       make k8s-wait"
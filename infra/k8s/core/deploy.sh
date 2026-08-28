#!/usr/bin/env bash
# Поднимает стенд в кластере: namespace aga, RBAC ядра, PVC, конфиги
# (roles + env + realm), Deployment ядра и Keycloak, сервисы. Идемпотентен —
# повторный запуск пересоздаёт только конфиги и приводит манифесты к желаемому.
#
# Конфиги не лежат в git: roles.yaml берётся из config/roles.yaml (sso-блок
# заменяется на стендовый), env — из .env, realm — тестовый из этого каталога.
set -euo pipefail

NS="${AGA_K8S_NAMESPACE:-aga}"
KUBECTL="${AGA_K8S_KUBECTL:-kubectl}"
ROLES_SRC="${AGA_ROLES_CONFIG:-./config/roles.yaml}"
LLM_API_URL="${AGA_K8S_LLM_API_URL:-http://192.168.49.1:11434/v1}"
LLM_API_KEY="${LLM_API_KEY:-}"
RUST_LOG="${RUST_LOG:-info}"
PORT="${PORT:-8080}"
REALM="aga"
CLIENT_SECRET="aga-secret"

# Куда браузер пойдёт логиниться: внешний адрес Keycloak (NodePort).
# Для minikube — адрес кластера; в других случаях переопределить KEYCLOAK_URL.
EXT_IP="localhost"
if command -v minikube >/dev/null 2>&1 && minikube status >/dev/null 2>&1; then
  EXT_IP=$(minikube ip)
fi
KEYCLOAK_URL="${KEYCLOAK_URL:-http://${EXT_IP}:30081}"

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
# env ядра: LLM (кластер-достижимый адрес!), порт, лог-уровень.
$KUBECTL create configmap aga-env -n "$NS" \
  --from-literal=LLM_API_URL="$LLM_API_URL" \
  --from-literal=LLM_API_KEY="$LLM_API_KEY" \
  --from-literal=RUST_LOG="$RUST_LOG" \
  --from-literal=PORT="$PORT" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

# roles.yaml ядра: роли из config/roles.yaml, sso-блок — стендовый (включён,
# указывает на Keycloak в кластере). authorize_url — внешний для браузера.
awk '/^sso:/{exit} {print}' "$ROLES_SRC" > "$WORK/roles.yaml"
cat >> "$WORK/roles.yaml" <<EOF
sso:
  enabled: true
  jwks_url: http://keycloak:8080/realms/${REALM}/protocol/openid-connect/certs
  authorize_url: ${KEYCLOAK_URL}/realms/${REALM}/protocol/openid-connect/auth
  token_url: http://keycloak:8080/realms/${REALM}/protocol/openid-connect/token
  client_id: aga
  client_secret: ${CLIENT_SECRET}
EOF
$KUBECTL create configmap aga-roles -n "$NS" \
  --from-file=roles.yaml="$WORK/roles.yaml" \
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
echo "    web client: $(minikube service aga -n "$NS" --url 2>/dev/null || echo 'minikube service aga -n aga')"
echo "    keycloak:   $KEYCLOAK_URL"
echo "    wait:       make k8s-wait"
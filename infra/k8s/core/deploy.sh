#!/usr/bin/env bash
# Поднимает стенд в кластере: namespace aga, RBAC ядра и рантайма, kubeconfig,
# PVC, конфиги и секреты (roles + env + realm), Deployment ядра, рантайма,
# Keycloak и Centrifugo, сервисы, ingress, NetworkPolicy воркстейшнов.
# Идемпотентен — повторный запуск обновляет конфиги и приводит манифесты к
# желаемому.
#
# Конфиги не лежат в git: roles.yaml собирается из main/config/roles.yaml
# (infra/stand-roles.sh: centrifuge- и sso-блоки — стендовые), секреты — из
# .env (AGA_CENTRIFUGO_*, AGA_KC_*; генерирует make init), realm — тестовый из
# этого каталога (секрет клиента и адреса — плейсхолдеры, Keycloak берёт их из
# env при импорте). Секреты попадают только в k8s Secret'ы.
#
# Манифесты ссылаются на образы `aga-*:latest` — при применении тег
# заменяется на AGA_IMAGE_TAG (git sha, infra/image-tag.sh). Плейсхолдеры
# __*_CONFIG_HASH__ — хеши конфигов: сменился конфиг — под пересоздаётся.
set -euo pipefail

NS="${AGA_K8S_NAMESPACE:-aga}"
KUBECTL="${AGA_K8S_KUBECTL:-kubectl}"
ROLES_SRC="${AGA_ROLES_CONFIG:-./main/config/roles.yaml}"
RUST_LOG="${RUST_LOG:-info}"
PORT="${PORT:-8080}"
# Origin веб-клиента в стенде: CORS ядра, возврат токена после SSO и
# post_logout_redirect_uri (Keycloak сверяет его строкой).
AGA_K8S_FRONT_URL="${AGA_K8S_FRONT_URL:-http://dev.localhost}"
# Публичный адрес ядра (ingress): redirect_uri SSO — <адрес>/auth/callback
# (ядро строит его из Host запроса).
AGA_K8S_API_URL="${AGA_K8S_API_URL:-http://api.localhost}"

# Куда браузер пойдёт логиниться. По умолчанию — auth.localhost (через ingress +
# локальный nginx-прокси из make k8s-dev, .localhost резолвится браузером в
# 127.0.0.1). Без ingress переопределить KEYCLOAK_URL, например
# http://<ip>:30081 (NodePort).
KEYCLOAK_URL="${KEYCLOAK_URL:-http://auth.localhost}"

K8S_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$K8S_DIR/../../.." && pwd)"
TAG="${AGA_IMAGE_TAG:-$(bash "$REPO_DIR/infra/image-tag.sh")}"
umask 077
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

for v in AGA_CENTRIFUGO_API_KEY AGA_CENTRIFUGO_SECRET AGA_KC_CLIENT_SECRET AGA_KC_ADMIN_PASSWORD; do
  [ -n "${!v:-}" ] || { echo "$v is not set — run 'make init' (adds stand secrets to .env)" >&2; exit 1; }
done

sha16() { sha256sum | cut -c1-16; }

echo "==> namespace"
$KUBECTL apply -f "$K8S_DIR/00-namespace.yaml"

echo "==> configmaps"
# env ядра: порт, лог-уровень. Дефолтной LLM из env нет — подключения к LLM
# живут в БД (создаются на странице «LLM» или сидом).
$KUBECTL create configmap aga-env -n "$NS" \
  --from-literal=AGA_FRONT_URL="$AGA_K8S_FRONT_URL" \
  --from-literal=RUST_LOG="$RUST_LOG" \
  --from-literal=PORT="$PORT" \
  --from-literal=AGA_MAX_THREAD_DEPTH="${AGA_MAX_THREAD_DEPTH:-10}" \
  --from-literal=AGA_MAX_THREAD_MESSAGES="${AGA_MAX_THREAD_MESSAGES:-100}" \
  --dry-run=client -o yaml > "$WORK/aga-env.yaml"
$KUBECTL apply -f "$WORK/aga-env.yaml"

# Адреса для плейсхолдеров realm: redirect_uri ядра и post-logout origin фронта.
$KUBECTL create configmap aga-keycloak-env -n "$NS" \
  --from-literal=AGA_KC_API_URL="$AGA_K8S_API_URL" \
  --from-literal=AGA_KC_FRONT_URL="$AGA_K8S_FRONT_URL" \
  --dry-run=client -o yaml > "$WORK/kc-env.yaml"
$KUBECTL apply -f "$WORK/kc-env.yaml"

# realm Keycloak: тестовые участники и клиент aga (секрет — плейсхолдер env).
$KUBECTL create configmap aga-keycloak-realm -n "$NS" \
  --from-file=realm.json="$K8S_DIR/keycloak-realm.json" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

echo "==> secrets"
# Секреты стенда из .env: ключи Centrifugo, секрет клиента и пароль админа
# Keycloak (secretKeyRef в 50/52-deployment). Значения — через файлы во
# временном каталоге (0700), не аргументами команды.
cat > "$WORK/stand.env" <<EOF
centrifugo-api-key=${AGA_CENTRIFUGO_API_KEY}
centrifugo-secret=${AGA_CENTRIFUGO_SECRET}
kc-client-secret=${AGA_KC_CLIENT_SECRET}
kc-admin-password=${AGA_KC_ADMIN_PASSWORD}
EOF
$KUBECTL create secret generic aga-stand -n "$NS" \
  --from-env-file="$WORK/stand.env" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

# roles.yaml ядра и рантайма: роли из main/config/roles.yaml, centrifuge- и
# sso-блоки — стендовые (Keycloak в кластере, authorize_url — внешний).
# В нём секреты — Secret aga-roles, монтируется в core/agent.
bash "$REPO_DIR/infra/stand-roles.sh" "$ROLES_SRC" "$KEYCLOAK_URL" > "$WORK/roles.yaml"
$KUBECTL create secret generic aga-roles -n "$NS" \
  --from-file=roles.yaml="$WORK/roles.yaml" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

# SSH-ключ aga (git+ssh для воркстейшнов): приватный задаёт админ в
# AGA_SSH_PRIVATE_KEY (из .env); не задан — пустой Secret, ядро работает без
# ключа. Монтируется в под как env (см. 30-deployment-core.yaml envFrom).
printf '%s' "${AGA_SSH_PRIVATE_KEY:-}" > "$WORK/ssh_key"
$KUBECTL create secret generic aga-ssh-env -n "$NS" \
  --from-file=AGA_SSH_PRIVATE_KEY="$WORK/ssh_key" \
  --dry-run=client -o yaml | $KUBECTL apply -f -

# Хеши конфигов для аннотаций подов: сменился конфиг — rollout.
KC_HASH="$(cat "$K8S_DIR/keycloak-realm.json" "$WORK/kc-env.yaml" "$WORK/stand.env" | sha16)"
CENTRIFUGO_HASH="$(grep '^centrifugo-' "$WORK/stand.env" | sha16)"
CORE_HASH="$(cat "$WORK/roles.yaml" "$WORK/aga-env.yaml" "$WORK/ssh_key" <(echo "$KC_HASH") | sha16)"

echo "==> manifests (image tag: $TAG)"
for f in "$K8S_DIR"/*.yaml; do
  sed -e "s|\(aga-[a-z]*\):latest|\1:${TAG}|g" \
      -e "s|__CORE_CONFIG_HASH__|${CORE_HASH}|g" \
      -e "s|__KEYCLOAK_CONFIG_HASH__|${KC_HASH}|g" \
      -e "s|__CENTRIFUGO_CONFIG_HASH__|${CENTRIFUGO_HASH}|g" \
      "$f" | $KUBECTL apply -f -
done

# roles.yaml раньше лежал в ConfigMap aga-roles — секретам там не место.
$KUBECTL delete configmap aga-roles -n "$NS" --ignore-not-found

echo "==> stand deployed: core + agent + Keycloak + Centrifugo in ns '$NS'"
echo "    images:     aga-core:$TAG, aga-workstation:$TAG"
echo "    keycloak:   $KEYCLOAK_URL"
echo "    front:      make k8s-deploy (infra/k8s/front) + make k8s-web"
echo "    wait:       make k8s-wait"
echo "    networkpolicy aga-workstation: enforced only by a CNI with NetworkPolicy support"

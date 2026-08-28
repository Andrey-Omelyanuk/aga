#!/usr/bin/env bash
# Интеграционная проверка уровня infra/k8s: воркстейшн как под.
#
# Требует:
#   - kubectl, текущий контекст указывает на кластер (minikube и подобные);
#   - docker на хосте для сборки образа воркстейшна;
#   - jq для разбора ответов API.
# LLM для проверки не нужен — проект/воркстейшн его не трогают.
#
# Проверяемые пункты истории 2026-08-28-workstation-kubernetes-pod:
#   1. созданный воркстейшн поднимает отдельный под с git-копией и Docker
#   4. проект регистрируется git-URL
#   5. воркстейшн работает на своей git-ветке
#   6. сервисы не пересекаются: у каждого воркстейшна свой под
#   7. агент в поде не имеет доступа к k8s API
#   8. локальный запуск (minikube) поднимает воркстейшны в кластере
set -euo pipefail

REPO="${REPO:-https://github.com/Andrey-Omelyanuk/aga}"
NS="${AGA_K8S_NAMESPACE:-default}"
PORT="${PORT:-18080}"
SERVER="http://localhost:${PORT}"
DB="$(mktemp -t aga-verify-XXXXXX.db)"
PID=""

cleanup() {
  if [ -n "$PID" ]; then kill "$PID" 2>/dev/null || true; fi
  rm -f "$DB" "$DB-wal" "$DB-shm"
}
trap cleanup EXIT

echo "==> kubectl: cluster check"
kubectl cluster-info >/dev/null

echo "==> build & load workstation image"
docker build -q -t aga-workstation:latest \
  -f infra/k8s/workstation-image/Dockerfile infra/k8s/workstation-image >/dev/null
if command -v minikube >/dev/null 2>&1; then
  minikube image load aga-workstation:latest
fi

echo "==> start server"
PORT="$PORT" \
AGA_DB_PATH="$DB" \
AGA_K8S_NAMESPACE="$NS" \
AGA_K8S_TEMPLATE="./infra/k8s/workstation-pod.yaml" \
AGA_K8S_WAIT_TIMEOUT=180 \
cargo run --quiet &
PID=$!
for _ in $(seq 1 90); do
  if curl -sf "$SERVER/users" >/dev/null 2>&1; then break; fi
  sleep 1
done

echo "==> project registered by git url"
PROJECT_ID=$(curl -sf -X POST "$SERVER/projects" \
  -H 'content-type: application/json' \
  -d "{\"git_url\": \"$REPO\"}" | jq -r '.id')
echo "project id=$PROJECT_ID"

echo "==> create workstation -> pod"
WS_ID=$(curl -sf -X POST "$SERVER/workstations" \
  -H 'content-type: application/json' \
  -d "{\"project_id\": $PROJECT_ID}" | jq -r '.id')
POD="ws-$WS_ID"
echo "workstation id=$WS_ID pod=$POD"

for _ in $(seq 1 60); do
  phase=$(kubectl get pod "$POD" -n "$NS" -o jsonpath='{.status.phase}' 2>/dev/null || true)
  [ "$phase" = "Running" ] && break
  sleep 2
done
[ "$(kubectl get pod "$POD" -n "$NS" -o jsonpath='{.status.phase}')" = "Running" ]

echo "==> pod has project copy and its own docker"
for _ in $(seq 1 30); do
  if kubectl exec -n "$NS" "$POD" -- sh -c \
      'ls /work/project >/dev/null && docker info >/dev/null' 2>/dev/null; then
    break
  fi
  sleep 2
done
kubectl exec -n "$NS" "$POD" -- sh -c 'ls /work/project >/dev/null && docker info >/dev/null'

echo "==> workstation works on its own branch"
BRANCH=$(kubectl exec -n "$NS" "$POD" -- sh -c 'git -C /work/project branch --show-current')
[ "$BRANCH" = "$POD" ]

echo "==> pod has no k8s API access"
[ "$(kubectl get pod "$POD" -n "$NS" -o jsonpath='{.spec.automountServiceAccountToken}')" = "false" ]
if kubectl exec -n "$NS" "$POD" -- sh -c \
    'test -f /var/run/secrets/kubernetes.io/serviceaccount/token'; then
  echo "FAIL: service account token mounted in pod" >&2
  exit 1
fi

echo "==> second workstation gets its own pod"
WS2_ID=$(curl -sf -X POST "$SERVER/workstations" \
  -H 'content-type: application/json' \
  -d "{\"project_id\": $PROJECT_ID}" | jq -r '.id')
[ "$WS2_ID" != "$WS_ID" ]
kubectl get pod "ws-$WS2_ID" -n "$NS" -o jsonpath='{.metadata.name}' >/dev/null

echo "==> delete workstation -> pod gone, not in list"
curl -sf -X DELETE "$SERVER/workstations/$WS_ID" >/dev/null
if kubectl get pod "$POD" -n "$NS" >/dev/null 2>&1; then
  echo "FAIL: pod still exists after delete" >&2
  exit 1
fi
curl -sf "$SERVER/workstations" | jq -e --argjson id "$WS_ID" \
  '[.[].id] | index($id) | not' >/dev/null

echo "==> OK"
#!/usr/bin/env bash
# Интеграционная проверка стенда в Kubernetes: ядро, Keycloak и воркстейшны
# поднимаются в кластере (minikube), ядро на хосте не запускается вовсе.
#
# Требует:
#   - kubectl на кластер (minikube и подобные);
#   - docker на хосте для сборки образов;
#   - jq для разбора ответов API.
# LLM не нужен — проект/воркстейшн его не трогают.
#
# Проверяемые пункты истории 2026-08-28-test-stand-in-k8s:
#   1. k8s-deploy: под ядра Ready, API отвечает на HTTP
#   2. ядро в кластере по git-URL создаёт воркстейшн — под ws-<id> готов
#   3. команды агента воркстейшна выполняются внутри его пода (из пода ядра)
#   4. данные ядра переживают перезапуск пода (PVC)
#   5. на стенде поднят Keycloak; у воркстейшна нет доступа к кластеру
#   6. недействительный токен отклоняется, действительный работает под участником
#   7. веб-клиент (front) и страница входа Keycloak отвечают извне
#   8. ядро работает в кластере, а не на хосте
#   9. docker compose из проекта убран
set -euo pipefail

REPO="${REPO:-https://github.com/Andrey-Omelyanuk/aga}"
NS="${AGA_K8S_NAMESPACE:-aga}"
KUBECTL="${AGA_K8S_KUBECTL:-kubectl}"
CORE_PORT="${CORE_PORT:-18080}"
KC_PORT="${KC_PORT:-18081}"
FRONT_PORT="${FRONT_PORT:-18082}"
SERVER="http://localhost:${CORE_PORT}"
KC_SERVER="http://localhost:${KC_PORT}"
FRONT_SERVER="http://localhost:${FRONT_PORT}"
EXT_IP="localhost"
PF_PIDS=""

cleanup() {
  if [ -n "$PF_PIDS" ]; then
    # shellcheck disable=SC2086
    kill $PF_PIDS 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "==> kubectl: cluster check"
$KUBECTL cluster-info >/dev/null

echo "==> build & load core + front + workstation images"
docker build -q -t aga-core:latest -f main/Dockerfile main >/dev/null
docker build -q -t aga-front:latest -f front/Dockerfile front >/dev/null
docker build -q -t aga-workstation:latest \
  -f infra/k8s/workstation-image/Dockerfile infra/k8s/workstation-image >/dev/null
if command -v minikube >/dev/null 2>&1; then
  minikube image load aga-core:latest aga-front:latest aga-workstation:latest
  EXT_IP=$(minikube ip)
fi

echo "==> bring up the stand (core + front + Keycloak in cluster)"
bash infra/k8s/core/deploy.sh
bash infra/k8s/front/deploy.sh

echo "==> core runs in cluster, not on host (8)"
$KUBECTL rollout status deploy/aga-core -n "$NS" --timeout=300s
$KUBECTL get pod -n "$NS" -l app=aga-core -o jsonpath='{.items[0].metadata.name}' >/dev/null

# Front (nginx) раздаёт SPA отдельно от ядра.
$KUBECTL rollout status deploy/aga-front -n "$NS" --timeout=300s
$KUBECTL get pod -n "$NS" -l app=aga-front -o jsonpath='{.items[0].metadata.name}' >/dev/null

# Повторный прогон: рестарт гарантирует свежий JWKS (ключи Keycloak могли
# смениться) и последний загруженный образ.
$KUBECTL rollout restart deploy/aga-core -n "$NS" >/dev/null
$KUBECTL rollout status deploy/aga-core -n "$NS" --timeout=300s

echo "==> port-forward core + keycloak + front"
$KUBECTL port-forward -n "$NS" svc/aga "$CORE_PORT":8080 >/dev/null 2>&1 &
PF_PIDS="$PF_PIDS $!"
$KUBECTL port-forward -n "$NS" svc/keycloak "$KC_PORT":8080 >/dev/null 2>&1 &
PF_PIDS="$PF_PIDS $!"
$KUBECTL port-forward -n "$NS" svc/aga-front "$FRONT_PORT":80 >/dev/null 2>&1 &
PF_PIDS="$PF_PIDS $!"

# Core больше не раздаёт статику: готовность — ответ API (401 без токена).
echo "==> core api answers"
for _ in $(seq 1 90); do
  code=$(curl -s -o /dev/null -w '%{http_code}' "$SERVER/users" 2>/dev/null || true)
  if [ "$code" = "401" ] || [ "$code" = "200" ]; then break; fi
  sleep 1
done
code=$(curl -s -o /dev/null -w '%{http_code}' "$SERVER/users")
[ "$code" = "401" ] || [ "$code" = "200" ]

echo "==> keycloak realm is up (5)"
for _ in $(seq 1 90); do
  if curl -sf "$KC_SERVER/realms/aga" >/dev/null 2>&1; then break; fi
  sleep 1
done
curl -sf "$KC_SERVER/realms/aga" | jq -e '.realm == "aga"' >/dev/null

echo "==> web client (front) answers, login page reachable from outside (7)"
for _ in $(seq 1 60); do
  if curl -sf "$FRONT_SERVER/" 2>/dev/null | grep -q '<main'; then break; fi
  sleep 2
done
curl -sf "$FRONT_SERVER/" | grep -q '<main'
KC_EXT="$EXT_IP:30081"
for _ in $(seq 1 60); do
  if curl -sf -o /dev/null \
      "http://${KC_EXT}/realms/aga/protocol/openid-connect/auth?client_id=aga&response_type=code&redirect_uri=${SERVER}/auth/callback&scope=openid"; then
    break
  fi
  sleep 2
done
curl -sf "http://${KC_EXT}/realms/aga/protocol/openid-connect/auth?client_id=aga&response_type=code&redirect_uri=${SERVER}/auth/callback&scope=openid" >/dev/null
# /auth/login ядра редиректит в Keycloak (вход через него).
[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Origin: $SERVER" "$SERVER/auth/login")" = "307" ]

echo "==> api without token is rejected, sso enabled (6)"
[ "$(curl -s -o /dev/null -w '%{http_code}' "$SERVER/users")" = "401" ]
[ "$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer invalid.token.x" "$SERVER/users")" = "401" ]

echo "==> CORS allows the front origin"
curl -s -D- -o /dev/null -H "Origin: http://dev.localhost" "$SERVER/users" \
  | grep -qi '^access-control-allow-origin: http://dev.localhost'

get_token() {
  local user="$1" pass="$2"
  curl -sf -X POST "$KC_SERVER/realms/aga/protocol/openid-connect/token" \
    -d grant_type=password \
    -d client_id=aga -d client_secret=aga-secret \
    -d "username=$user" -d "password=$pass" | jq -r '.access_token'
}

echo "==> participant token works (6)"
ALICE=$(get_token alice alice-pass)
[ -n "$ALICE" ]
curl -sf -H "Authorization: Bearer $ALICE" "$SERVER/users" >/dev/null
BOB=$(get_token bob bob-pass)
[ -n "$BOB" ]

echo "==> project registered by git url (2)"
PROJECT_ID=$(curl -sf -H "Authorization: Bearer $ALICE" -X POST "$SERVER/projects" \
  -H 'content-type: application/json' \
  -d "{\"git_url\": \"$REPO\"}" | jq -r '.id')
echo "project id=$PROJECT_ID"

echo "==> workstation creates its own pod (2)"
WS_ID=$(curl -sf -H "Authorization: Bearer $BOB" -X POST "$SERVER/workstations" \
  -H 'content-type: application/json' \
  -d "{\"project_id\": $PROJECT_ID}" | jq -r '.id')
POD="ws-$WS_ID"
echo "workstation id=$WS_ID pod=$POD"

for _ in $(seq 1 60); do
  phase=$($KUBECTL get pod "$POD" -n "$NS" -o jsonpath='{.status.phase}' 2>/dev/null || true)
  [ "$phase" = "Running" ] && break
  sleep 2
done
[ "$($KUBECTL get pod "$POD" -n "$NS" -o jsonpath='{.status.phase}')" = "Running" ]

echo "==> pod has project copy and its own docker"
for _ in $(seq 1 30); do
  if $KUBECTL exec -n "$NS" "$POD" -- sh -c \
      'ls /work/project >/dev/null && docker info >/dev/null' 2>/dev/null; then
    break
  fi
  sleep 2
done
$KUBECTL exec -n "$NS" "$POD" -- sh -c 'ls /work/project >/dev/null && docker info >/dev/null'

echo "==> workstation works on its own branch"
BRANCH_OK=""
for _ in $(seq 1 30); do
  if BRANCH=$($KUBECTL exec -n "$NS" "$POD" -- sh -c \
      'git -C /work/project branch --show-current' 2>/dev/null) && [ "$BRANCH" = "$POD" ]; then
    BRANCH_OK=1
    break
  fi
  sleep 2
done
[ -n "$BRANCH_OK" ]

echo "==> pod has no k8s API access (5)"
[ "$($KUBECTL get pod "$POD" -n "$NS" -o jsonpath='{.spec.automountServiceAccountToken}')" = "false" ]
if $KUBECTL exec -n "$NS" "$POD" -- sh -c \
    'test -f /var/run/secrets/kubernetes.io/serviceaccount/token'; then
  echo "FAIL: service account token mounted in pod" >&2
  exit 1
fi

echo "==> agent commands run in the workstation pod, from the core pod (3)"
OUT=$($KUBECTL exec -n "$NS" deploy/aga-core -- sh -c \
  "kubectl exec -n $NS $POD -- sh -c 'echo agent-in-pod'" 2>/dev/null)
[ "$OUT" = "agent-in-pod" ]

echo "==> second workstation gets its own pod"
WS2_ID=$(curl -sf -H "Authorization: Bearer $BOB" -X POST "$SERVER/workstations" \
  -H 'content-type: application/json' \
  -d "{\"project_id\": $PROJECT_ID}" | jq -r '.id')
[ "$WS2_ID" != "$WS_ID" ]
$KUBECTL get pod "ws-$WS2_ID" -n "$NS" -o jsonpath='{.metadata.name}' >/dev/null

echo "==> delete workstation -> pod gone, not in list"
curl -sf -H "Authorization: Bearer $BOB" -X DELETE "$SERVER/workstations/$WS_ID" >/dev/null
if $KUBECTL get pod "$POD" -n "$NS" >/dev/null 2>&1; then
  echo "FAIL: pod still exists after delete" >&2
  exit 1
fi
curl -sf -H "Authorization: Bearer $BOB" "$SERVER/workstations" | jq -e \
  --argjson id "$WS_ID" '[.[].id] | index($id) | not' >/dev/null

echo "==> core data survives pod restart (PVC, 4)"
$KUBECTL rollout restart deploy/aga-core -n "$NS"
$KUBECTL rollout status deploy/aga-core -n "$NS" --timeout=300s
for _ in $(seq 1 60); do
  if curl -sf -H "Authorization: Bearer $ALICE" "$SERVER/projects" >/dev/null 2>&1; then break; fi
  sleep 2
done
curl -sf -H "Authorization: Bearer $ALICE" "$SERVER/projects" | jq -e \
  --argjson id "$PROJECT_ID" '[.[].id] | index($id)' >/dev/null

echo "==> docker compose is gone (9)"
[ ! -f infra/compose.yml ]
if grep -qE '^\s*(run-d|stop|down|ps|log|reset|delete|sh):' makefile; then
  echo "FAIL: compose make targets still present" >&2
  exit 1
fi

echo "==> cleanup test workstations"
for _ in $(seq 1 10); do
  if curl -sf -H "Authorization: Bearer $BOB" -X DELETE "$SERVER/workstations/$WS2_ID" >/dev/null 2>&1; then
    break
  fi
  # Порт-форвард мог разорваться во время рестарта ядра — повторяем.
  sleep 2
done
for _ in $(seq 1 30); do
  if ! $KUBECTL get pod "ws-$WS2_ID" -n "$NS" >/dev/null 2>&1; then break; fi
  sleep 2
done
if $KUBECTL get pod "ws-$WS2_ID" -n "$NS" >/dev/null 2>&1; then
  echo "FAIL: ws-$WS2_ID pod still exists after delete" >&2
  exit 1
fi

echo "==> OK"
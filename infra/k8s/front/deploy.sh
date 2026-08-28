#!/usr/bin/env bash
# Поднимает веб-клиент в кластере: Deployment aga-front (nginx) + Service.
# Идемпотентен. Требует namespace aga (создаёт infra/k8s/core/deploy.sh).
set -euo pipefail

NS="${AGA_K8S_NAMESPACE:-aga}"
K8S_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

$KUBECTL create namespace "$NS" --dry-run=client -o yaml | $KUBECTL apply -f - 2>/dev/null || true
$KUBECTL apply -f "$K8S_DIR/30-deployment-front.yaml"
$KUBECTL apply -f "$K8S_DIR/40-service-front.yaml"

echo "==> front deployed: SPA at dev.localhost (nginx), api at api.localhost"
echo "    web client: $(minikube service aga-front -n "$NS" --url 2>/dev/null || echo 'minikube service aga-front -n aga')"
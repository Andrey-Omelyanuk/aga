#!/usr/bin/env bash
# Поднимает веб-клиент в кластере: Deployment aga-front (nginx) + Service.
# Идемпотентен. Требует namespace aga (создаёт infra/k8s/core/deploy.sh).
# Образ — aga-front с тегом AGA_IMAGE_TAG (git sha, infra/image-tag.sh):
# манифест ссылается на :latest, тег подставляется при применении.
set -euo pipefail

NS="${AGA_K8S_NAMESPACE:-aga}"
KUBECTL="${AGA_K8S_KUBECTL:-${KUBECTL:-kubectl}}"
K8S_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$K8S_DIR/../../.." && pwd)"
TAG="${AGA_IMAGE_TAG:-$(bash "$REPO_DIR/infra/image-tag.sh")}"

$KUBECTL create namespace "$NS" --dry-run=client -o yaml | $KUBECTL apply -f - 2>/dev/null || true
sed "s|aga-front:latest|aga-front:${TAG}|g" "$K8S_DIR/30-deployment-front.yaml" | $KUBECTL apply -f -
$KUBECTL apply -f "$K8S_DIR/40-service-front.yaml"

echo "==> front deployed (aga-front:$TAG): SPA at dev.localhost (nginx), api at api.localhost"
echo "    web client: $(minikube service aga-front -n "$NS" --url 2>/dev/null || echo 'minikube service aga-front -n aga')"

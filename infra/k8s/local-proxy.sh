#!/usr/bin/env bash
# Локальный nginx-прокси для доступа к стенду по *.localhost без
# `minikube tunnel` (в minikube v1.37 туннель не слушает 127.0.0.1:80).
# Контейнер с --network host слушает 80 (IPv4+IPv6) и проксирует всё на
# ingress-nginx по nodePort, сохраняя Host — маршрутизацию делает ingress.
#
# Usage: bash infra/k8s/local-proxy.sh [start|stop]
set -euo pipefail

NAME="aga-nginx-proxy"
IMAGE="${AGA_LOCAL_PROXY_IMAGE:-nginx:alpine}"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/local-proxy"
CONF_TEMPLATE="$DIR/nginx.conf"
RUN_DIR="${AGA_LOCAL_PROXY_RUN_DIR:-$DIR/run}"

start() {
  MINIKUBE_IP="$(minikube ip)"
  NODE_PORT="$(kubectl get svc ingress-nginx-controller -n ingress-nginx \
    -o jsonpath='{.spec.ports[0].nodePort}')"
  mkdir -p "$RUN_DIR"
  sed -e "s/__MINIKUBE_IP__/$MINIKUBE_IP/" \
      -e "s/__INGRESS_NODE_PORT__/$NODE_PORT/" \
      "$CONF_TEMPLATE" > "$RUN_DIR/nginx.conf"
  docker rm -f "$NAME" 2>/dev/null || true
  docker run -d --name "$NAME" --network host \
    -v "$RUN_DIR/nginx.conf:/etc/nginx/nginx.conf:ro" "$IMAGE" >/dev/null
  echo "==> nginx proxy: $NAME (-> $MINIKUBE_IP:$NODE_PORT)"
  echo "    http://dev.localhost   (web client)"
  echo "    http://api.localhost   (REST API, same backend)"
  echo "    http://auth.localhost  (Keycloak / SSO login)"
}

stop() {
  docker rm -f "$NAME" 2>/dev/null || true
  echo "==> nginx proxy stopped: $NAME"
}

case "${1:-start}" in
  start) start ;;
  stop) stop ;;
  *) echo "usage: $0 [start|stop]" >&2; exit 1 ;;
esac
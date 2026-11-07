#!/bin/sh
set -eu

# Подставляет адреса в index.html перед стартом nginx: API_ENDPOINT (ядро) и
# PUBSUB_ENDPOINT (websocket Centrifugo, например wss://pub-sub.example.com/connection/websocket).
# Пусто — фронт берёт fallback по hostname (dev.localhost / pub-sub.localhost).
# Значение попадает в JS-строку в одинарных кавычках: экранируем `\` и `'`,
# а для sed — ещё `|` и `&`.
escape() {
  printf '%s' "$1" | sed -e 's/\\/\\\\\\\\/g' -e "s/'/\\\\\\\\'/g" -e 's/[|&]/\\&/g'
}

INDEX=/usr/share/nginx/html/index.html
sed -i "s|<API_ENDPOINT>|$(escape "${API_ENDPOINT:-}")|g" "$INDEX"
sed -i "s|<PUBSUB_ENDPOINT>|$(escape "${PUBSUB_ENDPOINT:-}")|g" "$INDEX"

exec "$@"

#!/bin/sh

# Подставляет адрес ядра (API_ENDPOINT) в index.html перед стартом nginx.
sed -i "s|<API_ENDPOINT>|$API_ENDPOINT|g" /usr/share/nginx/html/index.html

exec "$@"
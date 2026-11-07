#!/usr/bin/env bash
# Секреты стенда в .env: недостающие переменные дописываются случайными
# значениями (openssl rand -hex 32). Идемпотентен — заданные значения не
# трогает, пустую строку `VAR=` заполняет на месте. Вызывается из `make init`.
#
# Usage: bash infra/env-secrets.sh [path/to/.env]
set -euo pipefail

ENV_FILE="${1:-.env}"
SECRETS=(AGA_CENTRIFUGO_API_KEY AGA_CENTRIFUGO_SECRET AGA_KC_CLIENT_SECRET AGA_KC_ADMIN_PASSWORD)

gen() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32
  else
    head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'
    echo
  fi
}

[ -f "$ENV_FILE" ] || { echo "env file not found: $ENV_FILE (run 'make init')" >&2; exit 1; }

for v in "${SECRETS[@]}"; do
  grep -q "^${v}=." "$ENV_FILE" && continue
  val="$(gen)"
  if grep -q "^${v}=\$" "$ENV_FILE"; then
    # Пустая строка из шаблона — заполняем на месте (cat сохраняет права файла).
    tmp="$(mktemp)"
    awk -v k="$v" -v val="$val" '$0 == k"=" {print k"="val; next} {print}' "$ENV_FILE" > "$tmp"
    cat "$tmp" > "$ENV_FILE"
    rm -f "$tmp"
  else
    # Файл может кончаться без перевода строки — не склеиваем с последней.
    [ -z "$(tail -c1 "$ENV_FILE")" ] || echo >> "$ENV_FILE"
    echo "${v}=${val}" >> "$ENV_FILE"
  fi
  echo "  + $v (generated)"
done

#!/usr/bin/env bash
# Тег образов k8s-стенда по состоянию git: короткий sha HEAD; при
# незакоммиченных правках — `<sha>-dirty-<хеш правок>`. Хеш нужен, чтобы разные
# грязные сборки не делили один тег: `minikube image load` не перезаписывает
# существующий тег, а IfNotPresent не тянет новый образ. Вне git — `dev`.
# Используют makefile (k8s-build/k8s-load), core/deploy.sh, front/deploy.sh,
# e2e/k8s.sh.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

sha="$(git rev-parse --short HEAD 2>/dev/null)" || { echo dev; exit 0; }
untracked="$(git ls-files --others --exclude-standard)"
if git diff --quiet HEAD -- && [ -z "$untracked" ]; then
  echo "$sha"
  exit 0
fi
dirty="$( { git diff HEAD --binary; git ls-files --others --exclude-standard -z | xargs -0 -r sha256sum; } \
  | sha256sum | cut -c1-7)"
echo "${sha}-dirty-${dirty}"

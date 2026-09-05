#!/bin/sh
# Воркстейшн — машина разработчика: свой Docker (DinD) и git-копия проекта.
# Про кластер под ничего не знает; доступ к k8s API ему не выдан.
set -e

# Собственный Docker-демон воркстейшна (DinD). dockerd работает root'ом внутри
# контейнера; команды агента ядро выполняет через `docker exec -u 1000`, поэтому
# файлы проекта в bind-mount принадлежат хостовому пользователю (uid 1000).
dockerd >/var/log/dockerd.log 2>&1 &
i=0
until docker info >/dev/null 2>&1; do
  i=$((i + 1))
  if [ "$i" -gt 120 ]; then
    echo "dockerd failed to start" >&2
    cat /var/log/dockerd.log >&2
    exit 1
  fi
  sleep 1
done

# Клон проекта и работа на своей ветке.
# SSH-ключ aga (git+ssh): в k8s монтируется k8s-Secret в /etc/secrets/<name>/,
# в dev-режиме ядро кладёт ключ в ~/.ssh напрямую (docker exec).
if ls /etc/secrets/*/id_ed25519 >/dev/null 2>&1; then
  mkdir -p ~/.ssh
  cp /etc/secrets/*/id_ed25519 ~/.ssh/id_ed25519
  chmod 600 ~/.ssh/id_ed25519
  printf 'IdentitiesOnly yes\nStrictHostKeyChecking accept-new\n' > ~/.ssh/config
fi
mkdir -p /work
# /work — рабочее дерево агента. В dev-режиме команды агента идут от uid 1000
# (docker exec -u 1000:1000): ему нужно создавать рядом с проектом временные
# каталоги клона (/work/project.new.<pid>) — /work должен принадлежать ему.
chown 1000:1000 /work
cd /work
if [ -n "$GIT_URL" ]; then
  if [ ! -d project ]; then
    git clone "$GIT_URL" project
  fi
  cd project
  if [ -n "$BRANCH" ]; then
    git checkout "$BRANCH" 2>/dev/null || git checkout -b "$BRANCH"
  fi
else
  # dev-режим: /work/project — отдельный volume (не bind-mount хоста), пустого
  # git-репо нет — инициализируем сами (раньше это делал make dev-prepare).
  # Проект агент наполняет сам. Владелец — uid/gid 1000 (хостовый пользователь
  # dev и команды агента через `docker exec -u 1000`).
  if [ ! -d project/.git ]; then
    git init -q project
    git -C project -c user.name=aga -c user.email=dev@aga commit -q --allow-empty -m init
    chown -R 1000:1000 project
  fi
fi

echo "workstation ready"
sleep infinity
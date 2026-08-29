# Docker compose как dev-стенд (ядро + 2 воркстейшна), k8s-совместимость

После перевода стенда целиком в k8s (2026-08-28-test-stand-in-k8s) для
локальной разработки не осталось способа проверить многопоточность
воркстейшнов без кластера: `make run` исполняет команды локальным `sh -c`,
воркстейшны-поды поднимаются только в minikube. Хочется вернуть docker compose
чисто для разработки — 2 воркстейшна рядом с ядром — не ломая стенд в k8s.

## Поведение
- dev-стенд поднимается `docker compose` без кластера: ядро + контейнеры ws-1 и ws-2
- воркстейшны в docker-режиме — контейнеры `ws-<id>` (тот же контракт имён, что поды),
  команды агента — `docker exec`; в k8s-режиме всё как было: поды, `kubectl exec`
- существующие контейнеры `ws-<id>` ядро переиспользует (compose поднимает их заранее),
  отсутствующие — запускает через `docker run`
- локальный путь в git_url (абсолютный путь хоста) монтируется в `/work/project`,
  git-URL клонируется как в k8s (GIT_URL/BRANCH)
- k8s-путь не меняется: `make k8s-*` и `verify.sh` работают как раньше, backend
  выбирается env-переменной `AGA_WS_BACKEND` (default k8s)
- dev-воркстейшны используют ту же машину-воркстейшн (DinD + git), что и поды

## Реализация
- `main/src/cluster.rs`: `Backend { K8s, Docker }`, `create_pod`/`delete_pod`/
  `wait_ready` разветвляются по backend; docker: `docker run -d --name ws-<id>
  --privileged` (bind-mount локального пути или GIT_URL/BRANCH), `docker rm -f`,
  поллинг `docker exec ... test -d /work/project/.git`; билдеры docker-команд
  вынесены для тестов.
- `main/src/agent.rs`: `Executor::DockerExec { container }` → `docker exec`.
  ВАЖНО: `docker exec` (в отличие от `kubectl exec`) не понимает разделитель `--`
  между контейнером и командой — docker 27 трактует `--` как команду и падает
  (поймано на dev-стенде, `exec: "--": executable file not found`).
- `main/src/workstation.rs`: `executor_for_workstation(ws_id, &Cluster)` выбирает
  executor по backend.
- `main/Dockerfile`: в образ ядра добавлен статический docker-клиент
  (стедж `docker:27-cli`) — в docker-режиме ядро управляет контейнерами
  через docker.sock хоста. ВАЖНО: `FROM docker:27-cli` нельзя вставлять внутрь
  builder-стежда — он оборвёт его (поймано: `COPY --from=builder
  /app/target/release/aga: not found`).
- `infra/dev-compose.yml`: сервисы `core` (docker.sock, `AGA_WS_BACKEND=docker`,
  `user: "0:0"` — доступ к сокету, `--env-file .env` для LLM_API_URL) и
  `ws-1`/`ws-2` (privileged, пустые git-репо в `main/data/work/ws-{1,2}`,
  проект агент наполняет сам). Имя проекта `aga-dev` — чтобы не конфликтовать
  с другими compose-проектами в `infra/` (поймано: collision container_name
  с чужим проектом `infra`).
- `makefile`: `dev-prepare` (пустые git-репо ws-{1,2} — контракт воркстейшна
  требует `/work/project/.git`, а тестовых проектов в репо нет; ранее examples/
  удалены), `dev-up`/`dev-down`/`dev-logs`/`dev-ps`/`dev-reset`/`dev-verify`.

## Итог
- `make dev-up && make dev-verify` — ядро отвечает на `/users`, ws-1/ws-2 ready
- через API: проект → POST /workstations (id 1,2 → контейнеры ws-1, ws-2
  переиспользованы, state ready) → сессии → реактивный `@Agent.docker-helper`
  выполняет команды внутри контейнера воркстейшна через `docker exec`
- `make build`, `make test` (46 тестов), `make lint` — без ошибок
- k8s-стенд не трогали: backend выбирается `AGA_WS_BACKEND`, дефолт — k8s;
  `make k8s-verify` остаётся эталоном стенда
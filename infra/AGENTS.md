# Infra

Образы ядра и фронта, dev-стенд (docker compose) и k8s-стенд (minikube): ядро,
веб-клиент, Keycloak и воркстейшны в одном кластере. Docker Compose в проекте
есть только для локальной разработки (`dev-compose.yml`), стенд — в k8s.

## Boundaries
- **Делает:** docker-образ ядра (`main/Dockerfile`, включая kubectl и docker CLI),
  docker-образ веб-клиента (`front/Dockerfile`, nginx), параметризация через `.env`,
  dev-стенд без кластера (`dev-compose.yml`: ядро + ws-1/ws-2), манифесты стенда
  (`k8s/core/`, `k8s/front/`), воркстейшны как поды Kubernetes (`k8s/`).
- **Не делает:** не содержит логики приложения (это `main/src/` и `front/`),
  не управляет наборами агентов (AgentSet — это `main/src/` и API).

## Architecture
- `main/Dockerfile` — мультистейдж-сборка бинарника ядра; в финальном образе
  `kubectl` (стенд — кластер), docker CLI (dev — контейнеры воркстейшнов) и бинарь.
  Статику ядро не раздаёт.
- `front/Dockerfile` — nginx, раздаёт `front/index.html` (отдельный сервис).
- `.env.example` — шаблон, копируется в корневой `.env` через `make init`.
- `dev-compose.yml` — dev-стенд: ядро (docker.sock, `AGA_WS_BACKEND=docker`) +
  2 воркстейшна (`ws-1`, `ws-2`, privileged, пустые git-репо в `main/data/work/`).
  Управление — `make dev-*`.
- `k8s/core/` — стенд ядра: манифесты ядра, Keycloak, RBAC, PVC, сервисы, ingress;
  `deploy.sh` собирает конфиги из `.env` и `main/config/roles.yaml` (см. `k8s/AGENTS.md`).
- `k8s/front/` — стенд веб-клиента: Deployment + Service nginx; `deploy.sh`.
- `k8s/` — воркстейшны как поды и интеграционная проверка (см. `k8s/AGENTS.md`).

## Non-Obvious Rules
- Рутовый `makefile` — единственный интерфейс: `make run` (локальный dev,
  cargo run в `main/`), `make run-front` (раздача `front/`), `make dev-*`
  (dev-стенд в compose), `make k8s-*` (стенд в кластере).
- Dev-стенд — опциональный, только для разработки; стенд поднимается в k8s.
  Compose-команды идут с `--env-file .env` (LLM_API_URL из корневого `.env`).
- Для стенда LLM берётся из `AGA_K8S_LLM_API_URL` (кластер-достижимый адрес),
  локальный `LLM_API_URL` из `.env` — для `make run` и dev-стенда.
- Стенд включает SSO: `deploy.sh` заменяет sso-блок `main/config/roles.yaml` на
  стендовый (Keycloak в кластере). Локальный `make run` и dev-стенд — без SSO.
- Веб-клиент разнесён с API по сервисам: `dev.localhost` → фронт,
  `api.localhost` → ядро (см. `k8s/core/70-ingress.yaml`).

## Verification
- `make init` — создаёт `.env` и `main/config/roles.yaml` из примеров.
- `make dev-up` + `make dev-verify` — dev-стенд поднят, ядро отвечает, ws-1/ws-2 ready.
- `make k8s-deploy` + `make k8s-wait` — стенд поднят, API отвечает.
- `make k8s-verify` — интеграционная проверка стенда в локальном кластере.
- Критерий: ядро отвечает на HTTP, фронт раздаёт SPA, воркстейшн поднимается
  подом в том же кластере (`make`-targets).
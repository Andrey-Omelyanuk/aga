# Infra

Образы ядра и фронта и k8s-стенд (minikube): ядро, веб-клиент, Keycloak и
воркстейшны в одном кластере. Docker Compose в проекте нет.

## Boundaries
- **Делает:** docker-образ ядра (`main/Dockerfile`, включая kubectl), docker-образ
  веб-клиента (`front/Dockerfile`, nginx), параметризация через `.env`,
  манифесты стенда (`k8s/core/`, `k8s/front/`), воркстейшны как поды Kubernetes
  (`k8s/`).
- **Не делает:** не содержит логики приложения (это `main/src/` и `front/`),
  не управляет ролями агентов (`main/roles/`).

## Architecture
- `main/Dockerfile` — мультистейдж-сборка бинарника ядра; в финальном образе
  только `kubectl` (управление кластером из пода) и бинарь. Статику ядро не раздаёт.
- `front/Dockerfile` — nginx, раздаёт `front/index.html` (отдельный сервис).
- `.env.example` — шаблон, копируется в корневой `.env` через `make init`.
- `k8s/core/` — стенд ядра: манифесты ядра, Keycloak, RBAC, PVC, сервисы, ingress;
  `deploy.sh` собирает конфиги из `.env` и `main/config/roles.yaml` (см. `k8s/AGENTS.md`).
- `k8s/front/` — стенд веб-клиента: Deployment + Service nginx; `deploy.sh`.
- `k8s/` — воркстейшны как поды и интеграционная проверка (см. `k8s/AGENTS.md`).

## Non-Obvious Rules
- Рутовый `makefile` — единственный интерфейс: `make run` (локальный dev,
  cargo run в `main/`), `make run-front` (раздача `front/`), `make k8s-*`
  (стенд в кластере). Compose нигде не используется.
- Для стенда LLM берётся из `AGA_K8S_LLM_API_URL` (кластер-достижимый адрес),
  локальный `LLM_API_URL` из `.env` — только для `make run`.
- Стенд включает SSO: `deploy.sh` заменяет sso-блок `main/config/roles.yaml` на
  стендовый (Keycloak в кластере). Локальный `make run` — без SSO.
- Веб-клиент разнесён с API по сервисам: `dev.localhost` → фронт,
  `api.localhost` → ядро (см. `k8s/core/70-ingress.yaml`).

## Verification
- `make init` — создаёт `.env` и `main/config/roles.yaml` из примеров.
- `make k8s-deploy` + `make k8s-wait` — стенд поднят, API отвечает.
- `make k8s-verify` — интеграционная проверка стенда в локальном кластере.
- Критерий: ядро отвечает на HTTP, фронт раздаёт SPA, воркстейшн поднимается
  подом в том же кластере (`make`-targets).
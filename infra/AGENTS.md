# Infra

Образ ядра и k8s-стенд (minikube): ядро, Keycloak и воркстейшны в одном
кластере. Docker Compose в проекте нет.

## Boundaries
- Делает: docker-образ ядра (`Dockerfile`, включая kubectl и веб-клиент),
  параметризация через `.env`, манифесты стенда (`k8s/core/`), воркстейшны как
  поды Kubernetes (`k8s/`).
- Не делает: не содержит логики приложения (это `src/`), не управляет ролями
  агентов (`roles/`).

## Architecture
- `Dockerfile` (в корне) — мультистейдж-сборка бинарника; в финальном образе
  `kubectl` (управление кластером из пода) и `static/` (веб-клиент).
- `.env.example` — шаблон, копируется в корневой `.env` через `make init`.
- `k8s/core/` — стенд: манифесты ядра, Keycloak, RBAC, PVC, сервисы;
  `deploy.sh` собирает конфиги из `.env` и `config/roles.yaml` (см. `k8s/AGENTS.md`).
- `k8s/` — воркстейшны как поды и интеграционная проверка (см. `k8s/AGENTS.md`).

## Non-Obvious Rules
- Рутовый `makefile` — единственный интерфейс: `make run` (локальный dev,
  cargo run) и `make k8s-*` (стенд в кластере). Compose нигде не используется.
- Для стенда LLM берётся из `AGA_K8S_LLM_API_URL` (кластер-достижимый адрес),
  локальный `LLM_API_URL` из `.env` — только для `make run`.
- Стенд включает SSO: `deploy.sh` заменяет sso-блок `config/roles.yaml` на
  стендовый (Keycloak в кластере). Локальный `make run` — без SSO.

## Verification
- `make init` — создаёт `.env` и `config/roles.yaml` из примеров.
- `make k8s-deploy` + `make k8s-wait` — стенд поднят, API отвечает.
- `make k8s-verify` — интеграционная проверка стенда в локальном кластере.
- Критерий: сервис отвечает на HTTP, воркстейшн поднимается подом в том же
  кластере (`make`-targets).
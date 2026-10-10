# Infra

Образы ядра и фронта, dev-стенд (docker compose) и k8s-стенд (minikube): ядро,
веб-клиент, Keycloak и воркстейшны в одном кластере. Docker Compose в проекте
есть только для локальной разработки (`dev-compose.yml`), стенд — в k8s.

## Boundaries
- **Делает:** docker-образ ядра (`main/Dockerfile`, включая kubectl и docker CLI),
  docker-образ веб-клиента (`front/Dockerfile`, nginx), параметризация через `.env`,
  dev-стенд без кластера (`dev-compose.yml`: ядро + агент-рантайм (`aga agent`) +
  Keycloak + веб-клиент + ws-1/ws-2, SSO как в стенде),
  манифесты стенда
  (`k8s/core/`, `k8s/front/`), воркстейшны как поды Kubernetes (`k8s/`).
- **Не делает:** не содержит логики приложения (это `main/src/` и `front/`),
  не управляет наборами агентов (AgentSet — это `main/src/` и API).

## Architecture
- `main/Dockerfile` — мультистейдж-сборка бинарника ядра; в финальном образе
  `kubectl` (стенд — кластер), docker CLI (dev — контейнеры воркстейшнов) и бинарь.
  Статику ядро не раздаёт.
- `front/Dockerfile` — nginx, раздаёт `front/dist` (отдельный сервис).
- `.env.example` — шаблон, копируется в корневой `.env` через `make init`.
- `dev-compose.yml` — dev-стенд одного инстанса (= Linux-пользователь; проект
  compose `-p aga-<user>`, имена контейнеров `${NAME_PREFIX}`): ядро
  (docker.sock, `AGA_WS_BACKEND=docker`), агент-рантайм (`agent`: тот же образ
  `aga-core:<user>`, команда `aga agent`, общий с ядром том БД `aga-data` и
  docker.sock — подписан на Centrifugo, агентов ядро не запускает),
  веб-клиент (`front`, vite dev-server с HMR, образ `node:22`, bind-mount
  `../front`, host-порт `${AGA_FRONT_PORT}:80`; прод-сборка nginx из `dist/` —
  отдельно, `make build` + k8s/front),
  Keycloak (SSO dev-стенда, тот же realm `aga`, host-порт
  `${KEYCLOAK_PORT}:8080`),
  nginx-прокси `*.localhost` (`proxy`, host-порт `${AGA_PROXY_PORT}` — порт
  инстанса, не `:80`; конфиг `dev-proxy/nginx.conf` генерируется
  `make dev-proxy` из шаблона) + 2 воркстейшна (`ws-1`, `ws-2`, privileged,
  контейнеры `${WS_PREFIX}ws-1`/`-ws-2`, пустые git-репо в отдельных named
  volumes `ws-1-data`/`ws-2-data` — на хосте файлов воркстейшнов нет) +
  маленькая LLM (`ollama`, модель до 1B `qwen3:0.6b` —
  тянется при старте; подключение к ней в БД создаёт сид (`make dev-seed`,
  адрес `ollama:11434/v1`) или админ вручную на странице «LLM») + Centrifugo (события чата: общий канал
  `common` для аутентифицированных (на него же подписан агент-рантайм) +
  каналы `chat:<id>`/`user:<id>`; включён
  unidirectional-SSE (`CENTRIFUGO_UNI_SSE=true`) — транспорт подписки
  агент-рантайма; секреты — `aga-api-key`/`aga-hmac-secret`,
  совпадают с центрифуго-блоком roles.yaml). Прокси маршрутизирует как ingress
  в k8s, но по хостам инстанса: `dev.<user>.localhost` → front,
  `api.<user>.localhost` → core, `auth.<user>.localhost` →
  Keycloak, `pub-sub.<user>.localhost` → centrifugo.
  Конфиг ядра — `infra/dev-roles.yaml` (генерируется `make dev-roles` из
  `main/config/roles.yaml`, sso-блок — стендовый, Keycloak этого compose).
  SSH-ключ aga (`AGA_SSH_PRIVATE_KEY`) пробрасывается ядру из `.env` и
  инжектится в контейнеры ws при подъёме станции.
  Управление — `make dev-*`.
- `k8s/core/` — стенд ядра: манифесты ядра, Keycloak, RBAC, PVC, сервисы, ingress;
  `deploy.sh` собирает конфиги из `.env` и `main/config/roles.yaml` (см. `k8s/AGENTS.md`).
- `k8s/front/` — стенд веб-клиента: Deployment + Service nginx; `deploy.sh`.
- `k8s/` — воркстейшны как поды и интеграционная проверка (см. `k8s/AGENTS.md`).

## Non-Obvious Rules
- Рутовый `makefile` — единственный интерфейс: `make run` (локальный dev,
  cargo run в `main/`), `make run-front` (vite dev, `front/`), `make dev-*`
  (dev-стенд в compose), `make k8s-*` (стенд в кластере).
- Dev-стенд — опциональный, только для разработки; стенд поднимается в k8s.
  Compose-команды идут с `--env-file .env` и `-p aga-<user>` (инстанс = Linux-
  пользователь): порты, префикс контейнеров и ws-контейнеров выводятся из
  `$USER` (см. `makefile`), ssh-ключ — из `.env`.
- LLM из env не читается вовсе (LLM_API_URL/LLM_API_KEY/LLM_MODEL и
  AGA_K8S_LLM_API_URL не используются): подключения к LLM живут в БД и
  выбираются на странице «LLM». Dev-стенд поднимает свою маленькую LLM
  (`ollama`); подключение к ней создаёт в БД сид (`make dev-seed`) или
  вручную. В k8s-стенде подключение к внешней LLM создаётся в БД вручную
  или сидом.
- Стенд включает SSO: `deploy.sh` заменяет sso-блок `main/config/roles.yaml` на
  стендовый (Keycloak в кластере). Локальный `make run` — без SSO (аноним-супер);
  dev-стенд — со SSO (Keycloak в compose, `make dev-roles` генерирует
  `infra/dev-roles.yaml`). Ядро при включённом SSO ждёт JWKS с ретраями и не
  стартует без него (анонимный доступ закрыт).
- Веб-клиент разнесён с API по сервисам: `dev.localhost` → фронт,
  `api.localhost` → ядро, `auth.localhost` → Keycloak, `pub-sub.localhost` →
  Centrifugo (см. `k8s/core/70-ingress.yaml`). В dev-compose — те же маршруты,
  но с хостом инстанса (`dev.<user>.localhost`, порт `${AGA_PROXY_PORT}`) —
  `dev-proxy/nginx.conf.template`.

## Verification
- `make init` — создаёт `.env` и `main/config/roles.yaml` из примеров.
- Проверка стенда и e2e — `make dev-verify`, `make dev-e2e`, `make k8s-verify`;
  скрипты и описание — `e2e/` (`e2e/README.md`).
- Вход в Keycloak — тестовые учётки `alice`/`alice-pass` (participant) и
  `bob`/`bob-pass` (admin); фиксированные `sso_subject` заданы в
  `k8s/core/keycloak-realm.json` и совпадают с участниками сида (`aga seed`).
- `make k8s-deploy` + `make k8s-wait` — стенд поднят, API отвечает.
- Критерий: ядро отвечает на HTTP, фронт раздаёт SPA, воркстейшн поднимается
  подом в том же кластере (`make`-targets).
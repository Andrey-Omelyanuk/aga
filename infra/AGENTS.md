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
- `env-secrets.sh` — секреты стенда в `.env` (`make init`): недостающие
  `AGA_CENTRIFUGO_API_KEY`, `AGA_CENTRIFUGO_SECRET`, `AGA_KC_CLIENT_SECRET`,
  `AGA_KC_ADMIN_PASSWORD` дописывает случайными (`openssl rand -hex 32`),
  заданные не трогает. В репо секретов нет.
- `stand-roles.sh` — roles.yaml ядра для стенда (dev и k8s): блоки из
  `main/config/roles.yaml` до `sso:` без centrifuge, плюс стендовые centrifuge
  и sso с секретами из env.
- `image-tag.sh` — тег образов k8s-стенда: `<git sha>` или
  `<sha>-dirty-<хеш правок>`.
- `dev-compose.yml` — dev-стенд: ядро (docker.sock, `AGA_WS_BACKEND=docker`),
  агент-рантайм (`agent`: тот же образ `aga-core:dev`, команда `aga agent`,
  общий с ядром том БД `aga-data` и docker.sock — подписан на Centrifugo,
  агентов ядро не запускает),
  веб-клиент (`front`, vite dev-server с HMR, образ `node:22`, bind-mount
  `../front`, host-порт `${AGA_FRONT_PORT:-8081}:80`; прод-сборка nginx из
  `dist/` — отдельно, `make build` + k8s/front),
  Keycloak (SSO dev-стенда, тот же realm `aga`, host-порт
  `${KEYCLOAK_PORT:-8082}:8080`, админ `admin` / `AGA_KC_ADMIN_PASSWORD`),
  nginx-прокси `*.localhost` (`proxy`, `dev-proxy/nginx.conf`, host-порт
  `${AGA_PROXY_PORT:-80}`) + 2 воркстейшна (`ws-1`, `ws-2`, privileged, пустые
  git-репо в отдельных named volumes `ws-1-data`/`ws-2-data` — на хосте файлов
  воркстейшнов нет) + маленькая LLM (`ollama`, модель до 1B `qwen3:0.6b` —
  тянется при старте; подключение к ней в БД создаёт сид (`make dev-seed`,
  адрес `ollama:11434/v1`) или админ вручную на странице «LLM») + Centrifugo (события чата: общий канал
  `common` для аутентифицированных (на него же подписан агент-рантайм) +
  каналы `chat:<id>`/`user:<id>`; включён
  unidirectional-SSE (`CENTRIFUGO_UNI_SSE=true`) — транспорт подписки
  агент-рантайма; ключи — `AGA_CENTRIFUGO_*` из `.env`, те же попадают в
  центрифуго-блок dev-roles.yaml). Прокси маршрутизирует как ingress
  в k8s: `dev.localhost` → front, `api.localhost` → core, `auth.localhost` →
  Keycloak, `pub-sub.localhost` → centrifugo.
  Все host-порты опубликованы только на `127.0.0.1` — стенд не виден из LAN.
  Конфиг ядра — `infra/dev-roles.yaml` (0600, генерируется `make dev-roles`
  через `stand-roles.sh` из `main/config/roles.yaml`: centrifuge- и sso-блоки —
  стендовые, секреты из `.env`, Keycloak этого compose).
  SSH-ключ aga (`AGA_SSH_PRIVATE_KEY`) пробрасывается ядру из `.env` и
  инжектится в контейнеры ws при подъёме станции.
  Управление — `make dev-*`.
- `k8s/core/` — стенд ядра: манифесты ядра, агент-рантайма, Keycloak,
  Centrifugo, RBAC, PVC, сервисы, ingress, NetworkPolicy воркстейшнов;
  `deploy.sh` собирает конфиги из `.env` и `main/config/roles.yaml`, секреты
  кладёт в k8s Secret'ы (см. `k8s/AGENTS.md`).
- `k8s/front/` — стенд веб-клиента: Deployment + Service nginx; `deploy.sh`.
- `k8s/` — воркстейшны как поды и интеграционная проверка (см. `k8s/AGENTS.md`).

## Non-Obvious Rules
- Рутовый `makefile` — единственный интерфейс: `make run` (локальный dev,
  cargo run в `main/`), `make run-front` (vite dev, `front/`), `make dev-*`
  (dev-стенд в compose), `make k8s-*` (стенд в кластере).
- Dev-стенд — опциональный, только для разработки; стенд поднимается в k8s.
  Compose-команды идут с `--env-file .env` (ssh-ключ, порты, секреты стенда);
  без секретов (`${VAR:?}`) compose падает с подсказкой `run make init`.
- Секреты стенда живут только в `.env` (генерирует `make init`) и в
  производных: `infra/dev-roles.yaml` (0600, в .gitignore), env контейнеров
  compose, k8s Secret'ы `aga-stand` / `aga-roles` / `aga-ssh-env`. Литералов в
  репо нет; тестовые учётки realm (`alice`/`bob`) — фикстура e2e, не секрет.
- Адреса стенда сравниваются строкой: Keycloak пускает только redirect_uri
  `<api>/auth/callback` и post-logout `<front>`; при порте 80
  браузер пишет адрес без `:80`, поэтому makefile считает
  `AGA_DEV_URL_PORT` (пусто при 80, иначе `:<порт>`) и compose собирает из
  него origin фронта, redirect_uri, KC_HOSTNAME и allowed origins Centrifugo.
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
- Изоляция воркстейшна — см. раздел ниже; не считать станцию песочницей
  уровня ВМ.
- Веб-клиент разнесён с API по сервисам: `dev.localhost` → фронт,
  `api.localhost` → ядро, `auth.localhost` → Keycloak, `pub-sub.localhost` →
  Centrifugo (см. `k8s/core/70-ingress.yaml`). В dev-compose те же маршруты —
  `dev-proxy/nginx.conf`.

## Изоляция воркстейшна (как есть)
- Под/контейнер станции — `privileged` (ради DinD). Privileged-контейнер —
  не граница безопасности: из него достижимы устройства и ядро узла.
- `kubectl exec` идёт от root пода (в dev — `docker exec -u 0`): root строит
  песочницу, сама команда агента — от пользователя станции `aga` (uid 1000).
  Служебные команды ядра (клон, переключение проекта) в k8s — тоже от root
  пода (в dev — от uid 1000).
- Docker-демон станции (DinD) общий для всех агентов станции; доступ к нему
  равносилен root в станции.
- Граница между агентами — песочница владения (mount namespace: проект только
  на чтение, своя папка — на запись). Docker станции ей не закрыт: агент с
  `docker` в инструментах обходит её.
- Граница станция ↔ кластер: у пода нет токена SA
  (`automountServiceAccountToken: false`) и есть NetworkPolicy
  `aga-workstation` (вход запрещён, выход — DNS и интернет без приватных
  сетей) — только если CNI её применяет (minikube по умолчанию — нет, см.
  `k8s/AGENTS.md`). В dev-compose станции в общей сети compose: сетевой
  границы с ядром, Keycloak и Centrifugo нет.
- Открытый вопрос: замена privileged DinD на rootless Docker или sysbox.

## Verification
- `make init` — создаёт `.env` и `main/config/roles.yaml` из примеров и
  дописывает недостающие секреты стенда (повторный запуск ничего не меняет).
- Проверка стенда и e2e — `make dev-verify`, `make dev-e2e`, `make k8s-verify`;
  скрипты и описание — `e2e/` (`e2e/README.md`).
- `make dev-verify` проверяет и границы: порты только на loopback, Keycloak
  принимает redirect_uri ядра и отвергает чужой.
- Вход в Keycloak — тестовые учётки `alice`/`alice-pass` (participant) и
  `bob`/`bob-pass` (admin); фиксированные `sso_subject` заданы в
  `k8s/core/keycloak-realm.json` и совпадают с участниками сида (`aga seed`).
- `make k8s-deploy` (сам ждёт rollout core/agent/front) — стенд поднят, API
  отвечает.
- Критерий: ядро отвечает на HTTP, фронт раздаёт SPA, воркстейшн поднимается
  подом в том же кластере (`make`-targets).
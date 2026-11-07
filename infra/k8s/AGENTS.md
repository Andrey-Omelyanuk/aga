# infra/k8s — Стенд и воркстейшны как поды Kubernetes

## Overview
Уровень описывает тестовый стенд целиком в Kubernetes: ядро, агент-рантайм
(`aga agent`), веб-клиент (front)
и Keycloak поднимаются в кластере (minikube), воркстейшны — поды рядом.
Ядро на хосте не запускается. Для локальной разработки есть отдельный
dev-стенд без кластера — docker compose (`infra/dev-compose.yml`, `make dev-*`).

## Boundaries
- **Делает:** манифесты стенда (`core/`): ядро, агент-рантайм, Keycloak,
  Centrifugo, RBAC, kubeconfig, PVC, сервисы, тестовый realm, ingress,
  NetworkPolicy воркстейшнов; манифесты веб-клиента (`front/`): Deployment и Service
  nginx; образ машины-воркстейшна (DinD + git); шаблон воркстейшн-пода;
  скрипты развёртывания (`core/deploy.sh`, `front/deploy.sh`). Проверка
  стенда в кластере — `e2e/k8s.sh` (см. `e2e/README.md`).
- **Не делает:** не содержит логики ядра (это `main/src/`, модуль `cluster.rs`);
  не содержит кода веб-клиента (это `front/`); не собирает образы
  (`main/Dockerfile`, `front/Dockerfile`).

## Tech Stack
- Kubernetes, kubectl, minikube (локальный кластер), Docker (сборка образов).

## Architecture
```
infra/k8s/
├── AGENTS.md                 # этот файл
├── workstation-pod.yaml      # шаблон пода воркстейшна (рендерит ядро)
├── workstation-image/        # образ машины-воркстейшна (DinD + git)
│   ├── Dockerfile
│   └── entrypoint.sh
├── core/                     # стенд: ядро + агент-рантайм + Keycloak + ingress
│   ├── deploy.sh             # собирает конфиги и применяет манифесты
│   ├── 00-namespace.yaml     # ns aga
│   ├── 10-rbac.yaml          # SA + Role/RoleBinding: aga-core (поды ws) и aga-agent (только exec)
│   ├── 15-kubeconfig.yaml    # ConfigMap kubeconfig: tokenFile своего SA, ns aga
│   ├── 20-pvc.yaml           # БД ядра (trace.db) — на PVC (общая с рантаймом)
│   ├── 30-deployment-core.yaml   # Recreate, пробы /healthz
│   ├── 35-deployment-agent.yaml  # aga agent: Recreate, на узле ядра, liveness по heartbeat
│   ├── 40-service-core.yaml  # NodePort 30080 (API)
│   ├── 50-deployment-keycloak.yaml
│   ├── 60-service-keycloak.yaml  # NodePort 30081 (вход в браузере)
│   ├── 52-deployment-centrifugo.yaml  # Centrifugo (реальное время для чата)
│   ├── 54-service-centrifugo.yaml     # NodePort 30083
│   ├── 70-ingress.yaml       # dev.localhost→front, api.localhost→core, auth.localhost→Keycloak, pub-sub.localhost→centrifugo
│   ├── 80-networkpolicy-workstation.yaml  # ws: вход закрыт, выход — DNS + интернет
│   └── keycloak-realm.json   # тестовый realm (участники alice/bob), секрет и адреса — плейсхолдеры env
└── front/                    # стенд веб-клиента
    ├── deploy.sh
    ├── 30-deployment-front.yaml  # nginx, раздаёт SPA
    └── 40-service-front.yaml     # NodePort 30082 (веб-клиент)
```

## Patterns
- Шаблон воркстейшн-пода рендерит ядро (`main/src/cluster.rs`) подстановкой
  `{{POD_NAME}}`, `{{GIT_URL}}`, `{{BRANCH}}`, `{{IMAGE}}`. Встроенный в модуль
  дефолт совпадает с этим файлом — ядро работает даже без файла рядом.
- Образ воркстейшна — машина разработчика: свой Docker-демон (DinD) и клон
  проекта; про кластер под ничего не знает. В образе есть пользователь `aga`
  (uid 1000, home `/home/aga`): в dev-режиме ядро выполняет команды агента
  через `docker exec -u 1000:1000`, и этому uid нужны свой home (SSH-конфиг
  агента от `inject_ssh_key`) и права на `/work` (временный каталог клона при
  switch). В k8s-режиме команды идут root'ом — пользователь не мешает.
- Конфиги стенда не лежат в git: `deploy.sh` собирает их на лету — roles.yaml
  из `main/config/roles.yaml` (`infra/stand-roles.sh`: centrifuge- и sso-блоки
  стендовые), env ядра из `.env` (через `AGA_K8S_*`), realm из
  `keycloak-realm.json` (тестовый; секрет клиента и адреса — плейсхолдеры).
- Секреты — только в Secret'ах, из `.env` (`make init` генерирует):
  `aga-stand` (`centrifugo-api-key`, `centrifugo-secret`, `kc-client-secret`,
  `kc-admin-password` — secretKeyRef Centrifugo и Keycloak), `aga-roles`
  (roles.yaml ядра и рантайма — в нём те же секреты), `aga-ssh-env`
  (SSH-ключ). Значения `deploy.sh` передаёт kubectl файлами, не аргументами.
  Старый ConfigMap `aga-roles` deploy.sh удаляет.
- Образы — тег по git sha (`infra/image-tag.sh`; грязное дерево —
  `<sha>-dirty-<хеш правок>`): `make k8s-build`/`k8s-load` собирают и грузят
  `aga-core`, `aga-front`, `aga-workstation` с ним, `deploy.sh` при применении
  меняет `aga-*:latest` в манифестах на этот тег (и `AGA_K8S_IMAGE` ядра —
  образ воркстейшн-подов). Новый тег — новый pod template, поды обновляются
  сами; `make k8s-deploy` ждёт rollout core/agent/front.
- Аннотация `aga/config-hash` пода — хеш его конфигов (плейсхолдеры
  `__*_CONFIG_HASH__` подставляет `deploy.sh`): сменился roles.yaml, секрет или
  realm — под пересоздаётся (конфиги читаются при старте). Ядро и рантайм
  перезапускаются и при смене конфига Keycloak (новые ключи JWKS).

## Non-Obvious Rules
- Ядро и агент-рантайм в подах ходят в кластер через kubectl: в образе лежит
  `kubectl`, kubeconfig — ConfigMap `aga-kubeconfig` (`15-kubeconfig.yaml`,
  `KUBECONFIG=/etc/aga/kube/kubeconfig`): CA и `tokenFile` из projected-тома
  SA пода (kubectl перечитывает токен — ротация работает), namespace `aga` в
  контексте. Секретов в ConfigMap нет. Токен читает uid 1000: у всех
  контейнеров пода один `runAsUser` (pod securityContext), и kubelet отдаёт
  файл токена этому пользователю.
- RBAC — два SA в namespace `aga`. `aga-core` (HTTP-сервер): `pods`
  (get/list/create/patch/delete — `kubectl apply` пода и удаление),
  `pods/exec` (create), `secrets`: create + get/patch/update/delete только
  `aga-ssh` (SSH-ключ для воркстейшнов, `kubectl apply`). `aga-agent`
  (рантайм): `pods` get/list и `pods/exec` create — только `kubectl exec` в
  станции. Воркстейшн-поды остаются без SA (`automountServiceAccountToken:
  false`, привилегированные — DinD).
- SQLite на RWO PVC общая у ядра и рантайма: оба Deployment — `strategy:
  Recreate` (старый под гасится до старта нового), рантайм привязан к узлу
  ядра (`podAffinity` required по `app=aga-core`, `kubernetes.io/hostname`).
- Пробы: ядро — startup/readiness/liveness на `GET /healthz` (публичный);
  рантайм HTTP не раздаёт — liveness exec: mtime `AGA_HEARTBEAT_FILE`
  (`/tmp/aga-agent.alive`, обновляется каждые 10 с) не старше 60 с.
- NetworkPolicy `aga-workstation` (`80-networkpolicy-workstation.yaml`,
  селектор `app: aga-workstation` из шаблона пода): входящие запрещены
  полностью; исходящие — DNS (kube-system, 53/UDP+TCP) и `0.0.0.0/0` кроме
  10/8, 172.16/12, 192.168/16, 100.64/10 и 169.254/16 — станция не ходит в
  ядро, Keycloak, Centrifugo, узлы и LAN. `kubectl exec` идёт через kubelet и
  политикой не режется. Работает только с CNI, который применяет
  NetworkPolicy: `make k8s-up` — `minikube start` без `--cni`, CNI по
  умолчанию (bridge/kindnet) её не применяет — манифест принимается, но
  ничего не ограничивает. Для проверки — `make k8s-up
  MINIKUBE_START_ARGS=--cni=calico`; `make k8s-verify` пишет WARN, если
  станция достаёт до ядра.
- SSH-ключ aga (`AGA_SSH_PRIVATE_KEY` из `.env`) попадает в под ядра через
  Secret `aga-ssh-env` (envFrom в deployment, создаёт `deploy.sh`); из него
  ядро делает Secret `aga-ssh` и монтирует в воркстейшн-поды (`/etc/secrets/`),
  entrypoint раскладывает ключ в `~/.ssh` до git-клона.
- Ядро при старте тянет JWKS из Keycloak; `wait-keycloak` initContainer ждёт
  готовности realm, иначе ядро поднялось бы без SSO.
- БД ядра — PVC с `fsGroup: 1000` (пользователь aga из образа пишет в том).
  Данные переживают перезапуск пода.
- Keycloak — `start-dev --import-realm`, realm из configmap. `KC_HOSTNAME_STRICT:
  false`, `sslRequired: none` — работает по NodePort/IP. Клиент `aga`:
  `redirectUris` — только `${AGA_KC_API_URL}/auth/callback`,
  `post.logout.redirect.uris` — `${AGA_KC_FRONT_URL}`, `webOrigins: ["+"]`,
  секрет — `${AGA_KC_CLIENT_SECRET}`; `directAccessGrantsEnabled` (парольный
  grant для тестов). Плейсхолдеры Keycloak 26 подставляет из env при импорте
  (`aga-keycloak-env`: `AGA_K8S_API_URL`/`AGA_K8S_FRONT_URL` из `.env`, по
  умолчанию `http://api.localhost`/`http://dev.localhost`; секрет — `aga-stand`).
  Незаданная переменная осталась бы строкой `${...}` — поэтому secretKeyRef
  обязательный. Сравнение строковое: `:80` и слэш в конце не нормализуются.
  Админ master — `admin` / `kc-admin-password`.
- authorize_url для браузера — внешний (по умолчанию `http://auth.localhost`
  через ingress + tunnel; при желании переопределить `KEYCLOAK_URL`, например
  `http://<ip>:30081` NodePort); jwks_url и token_url — ClusterIP `keycloak:8080`
  (их дергает только ядро).
- SPA и API разнесены: `dev.localhost` → сервис `aga-front` (nginx, порт 80),
  `api.localhost` → сервис `aga` (ядро, порт 8080). Ядро статику не раздаёт.
- Centrifugo (`pub-sub.localhost`): события чата в каналах `common` (веб +
  жизненные события), `chat:<id>` и `user:<id>` (автор). Включён unidirectional-SSE
  (`CENTRIFUGO_UNI_SSE`) — им пользуется `aga-agent` (35-deployment) для серверных
  подписок на каналы привязанных пользователей; токен рантайм подписывает сам тем
  же HMAC-секретом. Ключи Centrifugo — secretKeyRef на `aga-stand`, те же
  значения `deploy.sh` пишет в центрифуго-блок roles.yaml (оба — из
  `AGA_CENTRIFUGO_*` в `.env`). Веб-клиент берёт
  connection-JWT у ядра (`/connection-jwt/`), публикует ядро по HTTP API с api_key.
- Поды используют локально загруженные образы (`IfNotPresent` + `minikube
  image load`), иначе kubelet тянет из реестра. `minikube image load` не
  обновляет уже существующий тег — поэтому тег уникален для каждого состояния
  исходников (git sha + хеш правок), а не `latest`.
- `minikube service aga-front -n aga` — адрес веб-клиента (NodePort);
  страница входа Keycloak — `http://$(minikube ip):30081/realms/aga`.
- Ручной доступ без `/etc/hosts`: `make k8s-dev` включает ingress addon,
  деплоит `70-ingress.yaml` и поднимает локальный nginx-прокси
  (`infra/k8s/local-proxy.sh start`, Docker `--network host`). Прокси слушает
  только loopback — `127.0.0.1:80` и `[::1]:80` (из LAN стенд не виден) — и
  форвардит всё на ingress-nginx по nodePort, сохраняя `Host` —
  маршрутизацию делает ingress. Браузер сам резолвит `*.localhost` в
  127.0.0.1/::1 (RFC 6761), `minikube tunnel` не нужен (в v1.37 он не слушает
  loopback:80). Остановка — `make k8s-dev-stop` (`local-proxy.sh stop`).
  Итог: `http://dev.localhost` — SPA (front), `http://api.localhost` — API,
  `http://auth.localhost` — Keycloak. Из терминала `.localhost` ОС не резолвит —
  для curl нужен `--resolve dev.localhost:80:127.0.0.1`.

## Изоляция воркстейшна (как есть)
- Под станции `privileged` (DinD) — не граница безопасности: из него
  достижимы устройства и ядро узла. `kubectl exec` идёт от root пода (root
  строит песочницу, команда агента — от uid 1000), Docker станции общий для
  её агентов и равносилен root в поде.
- Граница между агентами — песочница владения (mount namespace), не Docker.
- Граница станция ↔ кластер — нет токена SA и NetworkPolicy (только при CNI с
  её поддержкой). Замена privileged DinD на rootless Docker/sysbox — открытый
  вопрос.

## Verification
- `make k8s-deploy` — стенд поднят (ждёт rollout core/agent/front), API отвечает.
- Проверка стенда в кластере — `make k8s-verify` (`e2e/k8s.sh`, описание —
  `e2e/README.md`).

## Dependencies
- Ядро aga (`main/src/cluster.rs`) — рендерит шаблон и управляет подами.
- kubectl, docker, jq на хосте проверки; minikube для локального кластера.
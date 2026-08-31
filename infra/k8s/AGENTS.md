# infra/k8s — Стенд и воркстейшны как поды Kubernetes

## Overview
Уровень описывает тестовый стенд целиком в Kubernetes: ядро, веб-клиент (front)
и Keycloak поднимаются в кластере (minikube), воркстейшны — поды рядом.
Ядро на хосте не запускается. Для локальной разработки есть отдельный
dev-стенд без кластера — docker compose (`infra/dev-compose.yml`, `make dev-*`).

## Boundaries
- **Делает:** манифесты стенда (`core/`): ядро, Keycloak, RBAC, PVC, сервисы,
  тестовый realm, ingress; манифесты веб-клиента (`front/`): Deployment и Service
  nginx; образ машины-воркстейшна (DinD + git); шаблон воркстейшн-пода;
  скрипты развёртывания (`core/deploy.sh`, `front/deploy.sh`) и интеграционную
  проверку (`verify.sh`).
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
├── core/                     # стенд: ядро + Keycloak + ingress
│   ├── deploy.sh             # собирает конфиги и применяет манифесты
│   ├── 00-namespace.yaml     # ns aga
│   ├── 10-rbac.yaml          # SA + Role/RoleBinding ядра
│   ├── 20-pvc.yaml           # БД ядра (trace.db) — на PVC
│   ├── 30-deployment-core.yaml
│   ├── 40-service-core.yaml  # NodePort 30080 (API)
│   ├── 50-deployment-keycloak.yaml
│   ├── 60-service-keycloak.yaml  # NodePort 30081 (вход в браузере)
│   ├── 70-ingress.yaml       # dev.localhost→front, api.localhost→core, auth.localhost→Keycloak
│   └── keycloak-realm.json   # тестовый realm (участники alice/bob)
├── front/                    # стенд веб-клиента
│   ├── deploy.sh
│   ├── 30-deployment-front.yaml  # nginx, раздаёт SPA
│   └── 40-service-front.yaml     # NodePort 30082 (веб-клиент)
└── verify.sh                 # интеграционная проверка (make k8s-verify)
```

## Patterns
- Шаблон воркстейшн-пода рендерит ядро (`main/src/cluster.rs`) подстановкой
  `{{POD_NAME}}`, `{{GIT_URL}}`, `{{BRANCH}}`, `{{IMAGE}}`. Встроенный в модуль
  дефолт совпадает с этим файлом — ядро работает даже без файла рядом.
- Образ воркстейшна — машина разработчика: свой Docker-демон (DinD) и клон
  проекта; про кластер под ничего не знает.
- Конфиги стенда не лежат в git: `deploy.sh` собирает их на лету — roles.yaml
  из `main/config/roles.yaml` (sso-блок заменяется на стендовый), env ядра из `.env`
  (через `AGA_K8S_*`), realm из `keycloak-realm.json` (тестовый, не секрет).

## Non-Obvious Rules
- Ядро в поде управляет кластером через kubectl: в образе ядра лежит `kubectl`,
  а `kubectl-context` initContainer рендерит kubeconfig из токена и CA своего
  ServiceAccount'а в emptyDir (`KUBECONFIG=/etc/aga/kube/kubeconfig`). Без
  явного kubeconfig RBAC к ядру не применится. initContainer ходит под root —
  SA-токен недоступен пользователю aga (uid 1000).
- RBAC ядра — минимум в namespace `aga`: `pods` (get/list/create/delete),
  `pods/exec` (create) и `secrets` (get/create/update/delete — Secret `aga-ssh`
  с SSH-ключом aga для воркстейшнов). Воркстейшн-поды остаются без SA
  (`automountServiceAccountToken: false`, привилегированные — DinD).
- SSH-ключ aga (`AGA_SSH_PRIVATE_KEY` из `.env`) попадает в под ядра через
  Secret `aga-ssh-env` (envFrom в deployment, создаёт `deploy.sh`); из него
  ядро делает Secret `aga-ssh` и монтирует в воркстейшн-поды (`/etc/secrets/`),
  entrypoint раскладывает ключ в `~/.ssh` до git-клона.
- Ядро при старте тянет JWKS из Keycloak; `wait-keycloak` initContainer ждёт
  готовности realm, иначе ядро поднялось бы без SSO.
- БД ядра — PVC с `fsGroup: 1000` (пользователь aga из образа пишет в том).
  Данные переживают перезапуск пода.
- Keycloak — `start-dev --import-realm`, realm из configmap. `KC_HOSTNAME_STRICT:
  false`, `sslRequired: none` — работает по NodePort/IP. Клиент `aga` c
  `redirectUris: ["*"]` и `directAccessGrantsEnabled` (парольный grant для тестов).
- authorize_url для браузера — внешний (по умолчанию `http://auth.localhost`
  через ingress + tunnel; при желании переопределить `KEYCLOAK_URL`, например
  `http://<ip>:30081` NodePort); jwks_url и token_url — ClusterIP `keycloak:8080`
  (их дергает только ядро).
- SPA и API разнесены: `dev.localhost` → сервис `aga-front` (nginx, порт 80),
  `api.localhost` → сервис `aga` (ядро, порт 8080). Ядро статику не раздаёт.
- Воркстейшн-поды используют локально загруженные образы (`IfNotPresent` +
  тег `latest` + `minikube image load`), иначе kubelet тянет из реестра.
  `minikube image load` не обновляет уже существующий тег — после пересборки
  образа его нужно перезагружать (удалить тег в кластере и загрузить заново,
  либо импортировать через `docker save | docker exec -i minikube docker load`).
- `verify.sh` форвардит порты ядра (18080), Keycloak (18081) и фронта (18082),
  чтобы не конфликтовать с рабочим сервером. Стенд после проверки остаётся
  поднятым.
- `minikube service aga-front -n aga` — адрес веб-клиента (NodePort);
  страница входа Keycloak — `http://$(minikube ip):30081/realms/aga`.
- Ручной доступ без `/etc/hosts`: `make k8s-dev` включает ingress addon,
  деплоит `70-ingress.yaml` и поднимает локальный nginx-прокси
  (`infra/k8s/local-proxy.sh start`, Docker `--network host`). Прокси слушает
  80 (IPv4+IPv6) и форвардит всё на ingress-nginx по nodePort, сохраняя `Host` —
  маршрутизацию делает ingress. Браузер сам резолвит `*.localhost` в
  127.0.0.1/::1 (RFC 6761), `minikube tunnel` не нужен (в v1.37 он не слушает
  loopback:80). Остановка — `make k8s-dev-stop` (`local-proxy.sh stop`).
  Итог: `http://dev.localhost` — SPA (front), `http://api.localhost` — API,
  `http://auth.localhost` — Keycloak. Из терминала `.localhost` ОС не резолвит —
  для curl нужен `--resolve dev.localhost:80:127.0.0.1`.

## Verification
- Интеграционный тест: `make k8s-verify` (или `bash infra/k8s/verify.sh`) —
  требует kubectl-контекст на кластер и docker на хосте. Поднимает стенд,
  проверяет пункты истории `2026-08-28-test-stand-in-k8s`: под ядра Ready и
  API отвечает; фронт раздаёт SPA; воркстейшн по git-URL поднимает под рядом;
  команды агента идут в под из пода ядра; данные переживают рестарт (PVC);
  Keycloak и вход через него; токены (недействительный отклоняется, участник
  работает); внешний доступ. Dev-стенд (compose) — отдельный уровень, на
  кластер не влияет.
- Критерий готовности: скрипт завершается с `==> OK`.

## Dependencies
- Ядро aga (`main/src/cluster.rs`) — рендерит шаблон и управляет подами.
- kubectl, docker, jq на хосте проверки; minikube для локального кластера.
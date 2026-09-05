PROJECT_NAME=aga

# Load environment from .env (created by `make init`)
ifneq (,$(wildcard .env))
	include .env
	export
endif

CARGO ?= cargo
KUBECTL ?= kubectl

# Стенд живёт в namespace aga; имена образов подхватываются из .env.
NS ?= aga
CORE_IMAGE ?= aga-core:latest
FRONT_IMAGE ?= aga-front:latest
WS_IMAGE ?= aga-workstation:latest
WAIT_TIMEOUT ?= 180
K8S_CORE = infra/k8s/core
K8S_FRONT = infra/k8s/front

# Cargo-команды выполняются в main/ (CWD ядра): пути конфига (.env) относительно него.
CARGO_IN_MAIN = cd main && $(CARGO)

.PHONY: help init build release test lint fmt fmt-fix run run-front \
        front-test storybook storybook-build \
        k8s-up k8s-down k8s-build k8s-load k8s-deploy k8s-wait \
        k8s-logs k8s-web k8s-dev k8s-dev-stop k8s-reset k8s-verify \
 dev-up dev-down dev-logs dev-ps dev-reset dev-verify dev-e2e \
        dev-roles dev-seed k8s-seed

help:
	@echo "init        - Copy example files to working config (.env, roles.yaml)"
	@echo "build       - Build debug binary (cargo build)"
	@echo "release     - Build release binary (cargo build --release)"
	@echo "test        - Run unit tests (cargo test)"
	@echo "lint        - Run clippy (cargo clippy --all-targets)"
	@echo "fmt         - Check formatting (cargo fmt --check)"
	@echo "fmt-fix     - Apply formatting"
	@echo "run         - Run core locally (cargo run in main/)"
	@echo "run-front   - Serve front locally (vite dev, port 8081)"
	@echo "front-test  - Run frontend unit tests (vitest)"
	@echo "storybook   - Run Storybook dev server"
	@echo "dev-roles   - Generate infra/dev-roles.yaml with SSO enabled (dev Keycloak)"
	@echo "dev-up      - Start dev stand (docker compose: core + Keycloak + ws-1 + ws-2)"
	@echo "dev-down    - Stop dev stand (docker compose down)"
	@echo "dev-logs    - Follow dev stand logs"
	@echo "dev-ps      - Show dev stand containers"
	@echo "dev-reset   - Recreate dev stand from scratch (fresh DB)"
	@echo "dev-verify  - Check dev stand: core API + both workstations ready"
	@echo "dev-e2e     - E2E on dev stand: free ws, switch to mobx-model-ui, agent answers"
	@echo "dev-seed    - Restore test dataset into dev stand DB (aga seed in core)"
	@echo "k8s-seed    - Restore test dataset into cluster DB (aga seed via kubectl exec)"
	@echo "k8s-up      - Start local cluster (minikube)"
	@echo "k8s-down    - Delete local cluster (minikube delete)"
	@echo "k8s-build   - Build core, front and workstation images"
	@echo "k8s-load    - Load images into minikube"
	@echo "k8s-deploy  - Bring up the stand in the cluster (core + front + Keycloak)"
	@echo "k8s-wait    - Wait until the core pod is ready"
	@echo "k8s-logs    - Follow core logs"
	@echo "k8s-web     - Open the web client in the browser"
	@echo "k8s-dev     - Access the stand via dev.localhost/api.localhost/auth.localhost (nginx proxy)"
	@echo "k8s-dev-stop - Stop the local nginx proxy (*.localhost access)"
	@echo "k8s-reset   - Recreate the stand from scratch"
	@echo "k8s-verify  - Run infra/k8s integration check against the cluster"

init:
	@if [ ! -f "./.env"                  ]; then cp ./infra/.env.example     ./.env; fi
	@if [ ! -f "./main/config/roles.yaml" ]; then mkdir -p ./main/config && cp ./main/config.example.yml ./main/config/roles.yaml; fi
	@echo "Env ready. Check .env and main/config/roles.yaml."

build:
	$(CARGO_IN_MAIN) build

release:
	$(CARGO_IN_MAIN) build --release

test:
	$(CARGO_IN_MAIN) test

front-test:
	cd front && npm test

storybook:
	cd front && npm run storybook

storybook-build:
	cd front && npm run build-storybook

lint:
	$(CARGO_IN_MAIN) clippy --all-targets -- -D warnings

fmt:
	$(CARGO_IN_MAIN) fmt --all -- --check

fmt-fix:
	$(CARGO_IN_MAIN) fmt --all

run: init
	$(CARGO_IN_MAIN) run

# Локальный дев-сервер фронта без стенда (API_BASE — адрес ядра).
run-front:
	cd front && npm run dev

# Dev-стенд (docker compose): ядро + маленькая LLM (ollama) + Keycloak + фронт +
# 2 воркстейшна, без кластера. Поднимает весь стенд: core при старте создаёт
# подключение к ollama и ставит его дефолтным (bootstrap, см. main.rs). --build
# пересоздаёт контейнеры при смене образов (в т.ч. ws-1/ws-2). Проекты — пустые
# репо в named volumes (entrypoint инициализирует их сам).
# Стенд — по-прежнему k8s (make k8s-*); compose только для разработки.
DEV_COMPOSE = infra/dev-compose.yml
DEV_COMPOSE_CMD = docker compose --env-file .env -f $(DEV_COMPOSE)

# Конфиг dev-стенда со включённым SSO: из main/config/roles.yaml (локальный,
# sso выключен) генерируется infra/dev-roles.yaml, где sso-блок заменён на
# dev-стендовый (Keycloak compose-стенда: jwks/token внутри сети, authorize —
# через auth.localhost). Подход тот же, что в k8s core/deploy.sh.
dev-roles:
	@if [ ! -f main/config/roles.yaml ]; then echo "roles config not found: main/config/roles.yaml (run 'make init')" >&2; exit 1; fi
	@awk '/^sso:/{exit} {print}' main/config/roles.yaml > infra/dev-roles.yaml
	@# Центрифуго-блок: из свежего roles.yaml (make init) он уже есть до sso;
	@# в старом — подставляем дефолты, совпадающие с сервисом centrifugo compose.
	@grep -q '^centrifuge:' infra/dev-roles.yaml || printf 'centrifuge:\n  api_url: http://centrifugo:8000\n  api_key: aga-api-key\n  secret: aga-hmac-secret\n  channel: common\n' >> infra/dev-roles.yaml
	@printf 'sso:\n  enabled: true\n  jwks_url: http://keycloak:8080/realms/aga/protocol/openid-connect/certs\n  authorize_url: http://auth.localhost/realms/aga/protocol/openid-connect/auth\n  token_url: http://keycloak:8080/realms/aga/protocol/openid-connect/token\n  end_session_url: http://auth.localhost/realms/aga/protocol/openid-connect/logout\n  client_id: aga\n  client_secret: aga-secret\n' >> infra/dev-roles.yaml
	@echo "dev-roles.yaml written (SSO enabled -> dev Keycloak)"

dev-up: dev-roles
	$(DEV_COMPOSE_CMD) up -d --build
	# Прокси монтирует nginx.conf bind'ом — compose не пересоздаёт его при смене
	# файла; reload подхватывает новую конфигурацию (в т.ч. auth.localhost).
	@docker exec aga-proxy nginx -s reload >/dev/null 2>&1 || true

dev-down:
	$(DEV_COMPOSE_CMD) down

dev-logs:
	$(DEV_COMPOSE_CMD) logs -f

dev-ps:
	$(DEV_COMPOSE_CMD) ps

dev-reset:
	$(DEV_COMPOSE_CMD) down -v
	rm -f main/data/trace.db main/data/trace.db-wal main/data/trace.db-shm
	$(MAKE) dev-up

dev-verify: dev-roles
	@echo "Waiting for Keycloak realm (auth.localhost)..."
	@for i in $$(seq 1 60); do \
		if curl -s --resolve auth.localhost:80:127.0.0.1 http://auth.localhost/realms/aga | grep -q '"realm"'; then break; fi; \
		sleep 2; \
	done
	@test "$$(curl -s -o /dev/null -w '%{http_code}' http://localhost:8080/users)" = "401" && echo "core SSO: anonymous rejected OK"
	@test "$$(curl -s -o /dev/null -w '%{http_code}' http://localhost:8080/auth/login)" = "307" && echo "core auth/login redirect OK"
	@docker exec ws-1 sh -c "test -d /work/project/.git" && echo "ws-1 OK"
	@docker exec ws-2 sh -c "test -d /work/project/.git" && echo "ws-2 OK"
	@echo "Waiting for small LLM model (ollama:qwen3:0.6b)..."
	@for i in $$(seq 1 60); do \
		if docker exec aga-ollama ollama list 2>/dev/null | grep -q 'qwen3:0.6b'; then break; fi; \
		sleep 2; \
	done
	@docker exec aga-ollama ollama list | grep -q 'qwen3:0.6b' && echo "ollama small LLM OK"
	@curl -fsS http://localhost:$${AGA_FRONT_PORT:-8081}/ >/dev/null && echo "front OK"
	@curl -s --resolve auth.localhost:80:127.0.0.1 http://auth.localhost/realms/aga | grep -q '"realm"' && echo "proxy auth.localhost (keycloak) OK"
	@curl -fsS --resolve dev.localhost:80:127.0.0.1 http://dev.localhost/ >/dev/null && echo "proxy dev.localhost OK"
	@test "$$(curl -s -o /dev/null -w '%{http_code}' --resolve api.localhost:80:127.0.0.1 http://api.localhost/users)" = "401" && echo "proxy api.localhost (SSO) OK"

# Восстановить тестовый набор в БД dev-стенда (контейнер aga-core).
# Пересобирает образ ядра — seed-подкоманда живёт в бинаре /app/aga.
dev-seed:
	$(DEV_COMPOSE_CMD) up -d --build
	docker exec aga-core /app/aga seed

# E2E всего рабочего цикла агента на dev-стенде (см. infra/dev-e2e.sh):
# свободный воркстейшн, switch на mobx-model-ui, сессия, @Agent.ui отвечает.
# --force-recreate core ws-1 ws-2 подхватывает свежие образы ядра и
# воркстейшнов (пользователь aga, права /work, ключ в /home/aga/.ssh), сид
# сбрасывает БД в детерминированное состояние.
dev-e2e: dev-roles
	$(DEV_COMPOSE_CMD) up -d --build --force-recreate core ws-1 ws-2
	docker exec aga-core /app/aga seed
	bash infra/dev-e2e.sh

# Восстановить тестовый набор в БД кластера (PVC). Образ ядра должен быть
# передеплоен с поддержкой seed (make k8s-build && make k8s-load && make k8s-deploy).
k8s-seed:
	$(KUBECTL) exec deploy/aga-core -n $(NS) -- /app/aga seed

k8s-up:
	minikube start

k8s-down:
	minikube delete

k8s-build:
	docker build -t $(CORE_IMAGE) -f main/Dockerfile main
	docker build -t $(FRONT_IMAGE) -f front/Dockerfile front
	docker build -t $(WS_IMAGE) -f infra/k8s/workstation-image/Dockerfile infra/k8s/workstation-image

k8s-load:
	minikube image load $(CORE_IMAGE) $(FRONT_IMAGE) $(WS_IMAGE)

k8s-deploy:
	bash $(K8S_CORE)/deploy.sh
	bash $(K8S_FRONT)/deploy.sh

k8s-wait:
	$(KUBECTL) rollout status deploy/aga-core -n $(NS) --timeout=$(WAIT_TIMEOUT)s

k8s-logs:
	$(KUBECTL) logs deploy/aga-core -n $(NS) -f

k8s-web:
	minikube service aga-front -n $(NS)

# Доступ по хостам *.localhost (браузер резолвит их в 127.0.0.1/::1 без
# /etc/hosts): dev.localhost — SPA (front), api.localhost — API (core),
# auth.localhost — Keycloak. minikube tunnel не слушает loopback:80, поэтому стенд
# выставляется локальным nginx-прокси (Docker, --network host): 80 ->
# ingress-nginx nodePort, Host сохраняется, маршрутизацию делает ingress.
# Остановка — k8s-dev-stop.
k8s-dev:
	minikube addons enable ingress
	@kubectl get svc -n ingress-nginx ingress-nginx-controller -o jsonpath='{.spec.type}' 2>/dev/null | grep -q LoadBalancer \
		|| kubectl patch svc ingress-nginx-controller -n ingress-nginx -p '{"spec":{"type":"LoadBalancer"}}'
	bash $(K8S_CORE)/deploy.sh
	bash $(K8S_FRONT)/deploy.sh
	bash infra/k8s/local-proxy.sh start

k8s-dev-stop:
	bash infra/k8s/local-proxy.sh stop

k8s-reset:
	$(KUBECTL) delete ns $(NS) --ignore-not-found
	bash $(K8S_CORE)/deploy.sh
	bash $(K8S_FRONT)/deploy.sh

k8s-verify:
	bash infra/k8s/verify.sh
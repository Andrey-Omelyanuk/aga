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
        k8s-up k8s-down k8s-build k8s-load k8s-deploy k8s-wait \
        k8s-logs k8s-web k8s-dev k8s-dev-stop k8s-reset k8s-verify \
        dev-prepare dev-up dev-down dev-logs dev-ps dev-reset dev-verify

help:
	@echo "init        - Copy example files to working config (.env, roles.yaml)"
	@echo "build       - Build debug binary (cargo build)"
	@echo "release     - Build release binary (cargo build --release)"
	@echo "test        - Run unit tests (cargo test)"
	@echo "lint        - Run clippy (cargo clippy --all-targets)"
	@echo "fmt         - Check formatting (cargo fmt --check)"
	@echo "fmt-fix     - Apply formatting"
	@echo "run         - Run core locally (cargo run in main/)"
	@echo "run-front   - Serve front/index.html locally (python3 -m http.server)"
	@echo "dev-prepare - Create empty git repos for ws-1/ws-2 (main/data/work/)"
	@echo "dev-up      - Start dev stand (docker compose: core + ws-1 + ws-2)"
	@echo "dev-down    - Stop dev stand (docker compose down)"
	@echo "dev-logs    - Follow dev stand logs"
	@echo "dev-ps      - Show dev stand containers"
	@echo "dev-reset   - Recreate dev stand from scratch (fresh DB)"
	@echo "dev-verify  - Check dev stand: core API + both workstations ready"
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

lint:
	$(CARGO_IN_MAIN) clippy --all-targets -- -D warnings

fmt:
	$(CARGO_IN_MAIN) fmt --all -- --check

fmt-fix:
	$(CARGO_IN_MAIN) fmt --all

run: init
	$(CARGO_IN_MAIN) run

# Локальная раздача фронта без стенда (API_BASE должен указывать на ядро).
run-front:
	python3 -m http.server 8081 --directory front

# Dev-стенд (docker compose): ядро + 2 воркстейшна, без кластера.
# Воркстейшны поднимаются заранее (ws-1/ws-2) и переиспользуются ядром в
# docker-режиме (AGA_WS_BACKEND=docker). Проекты — пустые репо (dev-prepare).
# Стенд — по-прежнему k8s (make k8s-*); compose только для разработки.
# --env-file .env — compose берёт LLM_API_URL и др. из корневого .env
# (по умолчанию искал бы .env рядом с compose-файлом).
DEV_COMPOSE = infra/dev-compose.yml
DEV_COMPOSE_CMD = docker compose --env-file .env -f $(DEV_COMPOSE)

# Воркстейшн — git-копия проекта (контракт: /work/project/.git). Тестовых
# проектов нет — пустое репо, агент сам наполняет проект в dev.
dev-prepare:
	rm -rf main/data/work/ws-1 main/data/work/ws-2
	mkdir -p main/data/work/ws-1 main/data/work/ws-2
	git -C main/data/work/ws-1 init -q
	git -C main/data/work/ws-1 -c user.name=aga -c user.email=dev@aga commit -q --allow-empty -m init
	git -C main/data/work/ws-2 init -q
	git -C main/data/work/ws-2 -c user.name=aga -c user.email=dev@aga commit -q --allow-empty -m init

dev-up: dev-prepare
	$(DEV_COMPOSE_CMD) up -d --build

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

dev-verify:
	@curl -fsS http://localhost:8080/users >/dev/null && echo "core OK"
	@docker exec ws-1 sh -c "test -d /work/project/.git" && echo "ws-1 OK"
	@docker exec ws-2 sh -c "test -d /work/project/.git" && echo "ws-2 OK"

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
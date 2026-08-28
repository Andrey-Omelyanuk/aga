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
WS_IMAGE ?= aga-workstation:latest
WAIT_TIMEOUT ?= 180
K8S_CORE = infra/k8s/core

.PHONY: help init build release test lint fmt fmt-fix run \
        k8s-up k8s-down k8s-build k8s-load k8s-deploy k8s-wait \
        k8s-logs k8s-web k8s-dev k8s-dev-stop k8s-reset k8s-verify

help:
	@echo "init        - Copy example files to working config (.env, roles.yaml)"
	@echo "build       - Build debug binary (cargo build)"
	@echo "release     - Build release binary (cargo build --release)"
	@echo "test        - Run unit tests (cargo test)"
	@echo "lint        - Run clippy (cargo clippy --all-targets)"
	@echo "fmt         - Check formatting (cargo fmt --check)"
	@echo "fmt-fix     - Apply formatting"
	@echo "run         - Run server locally (cargo run)"
	@echo "k8s-up      - Start local cluster (minikube)"
	@echo "k8s-down    - Delete local cluster (minikube delete)"
	@echo "k8s-build   - Build core and workstation images"
	@echo "k8s-load    - Load images into minikube"
	@echo "k8s-deploy  - Bring up the stand in the cluster (core + Keycloak)"
	@echo "k8s-wait    - Wait until the core pod is ready"
	@echo "k8s-logs    - Follow core logs"
	@echo "k8s-web     - Open the web client in the browser"
	@echo "k8s-dev     - Access the stand via dev.localhost/api.localhost/auth.localhost (nginx proxy)"
	@echo "k8s-dev-stop - Stop the local nginx proxy (*.localhost access)"
	@echo "k8s-reset   - Recreate the stand from scratch"
	@echo "k8s-verify  - Run infra/k8s integration check against the cluster"

init:
	@if [ ! -f "./.env"                ]; then cp ./infra/.env.example     ./.env; fi
	@if [ ! -f "./config/roles.yaml"   ]; then mkdir -p ./config && cp ./config.example.yml ./config/roles.yaml; fi
	@echo "Env ready. Check .env and config/roles.yaml."

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

lint:
	$(CARGO) clippy --all-targets -- -D warnings

fmt:
	$(CARGO) fmt --all -- --check

fmt-fix:
	$(CARGO) fmt --all

run: init
	$(CARGO) run

k8s-up:
	minikube start

k8s-down:
	minikube delete

k8s-build:
	docker build -t $(CORE_IMAGE) .
	docker build -t $(WS_IMAGE) -f infra/k8s/workstation-image/Dockerfile infra/k8s/workstation-image

k8s-load:
	minikube image load $(CORE_IMAGE) $(WS_IMAGE)

k8s-deploy:
	bash $(K8S_CORE)/deploy.sh

k8s-wait:
	$(KUBECTL) rollout status deploy/aga-core -n $(NS) --timeout=$(WAIT_TIMEOUT)s

k8s-logs:
	$(KUBECTL) logs deploy/aga-core -n $(NS) -f

k8s-web:
	minikube service aga -n $(NS)

# Доступ по хостам *.localhost (браузер резолвит их в 127.0.0.1/::1 без
# /etc/hosts): dev.localhost — SPA, api.localhost — API, auth.localhost —
# Keycloak. minikube tunnel не слушает loopback:80, поэтому стенд выставляется
# локальным nginx-прокси (Docker, --network host): 80 -> ingress-nginx nodePort,
# Host сохраняется, маршрутизацию делает ingress. Остановка — k8s-dev-stop.
k8s-dev:
	minikube addons enable ingress
	@kubectl get svc -n ingress-nginx ingress-nginx-controller -o jsonpath='{.spec.type}' 2>/dev/null | grep -q LoadBalancer \
		|| kubectl patch svc ingress-nginx-controller -n ingress-nginx -p '{"spec":{"type":"LoadBalancer"}}'
	bash $(K8S_CORE)/deploy.sh
	bash infra/k8s/local-proxy.sh start

k8s-dev-stop:
	bash infra/k8s/local-proxy.sh stop

k8s-reset:
	$(KUBECTL) delete ns $(NS) --ignore-not-found
	bash $(K8S_CORE)/deploy.sh

k8s-verify:
	bash infra/k8s/verify.sh
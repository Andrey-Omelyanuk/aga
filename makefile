PROJECT_NAME=aga

# Load environment from .env (created by `make init`)
ifneq (,$(wildcard .env))
	include .env
	export
endif

CARGO ?= cargo
DC = docker compose -f infra/compose.yml -p $(PROJECT_NAME)

.PHONY: help init build release test lint fmt fmt-fix run run-d stop down \
        ps log reset delete sh k8s-up k8s-down k8s-image k8s-verify

help:
	@echo "init       - Copy example files to working config (.env, roles.yaml)"
	@echo "build      - Build debug binary (cargo build)"
	@echo "release    - Build release binary (cargo build --release)"
	@echo "test       - Run unit tests (cargo test)"
	@echo "lint       - Run clippy (cargo clippy --all-targets)"
	@echo "fmt        - Check formatting (cargo fmt --check)"
	@echo "fmt-fix    - Apply formatting"
	@echo "run        - Run server locally (cargo run)"
	@echo "run-d      - Start service in Docker (docker compose up -d)"
	@echo "stop       - Stop Docker service"
	@echo "down       - Stop and remove Docker containers/networks"
	@echo "ps         - Show Docker service status"
	@echo "log        - Show Docker logs (s=<service>)"
	@echo "reset      - Recreate Docker containers from scratch"
	@echo "delete     - Remove all Docker containers, volumes, images, networks"
	@echo "sh         - Open shell in Docker service (s=<service> u=<user>)"
	@echo "k8s-up     - Start local cluster (minikube)"
	@echo "k8s-down   - Delete local cluster (minikube delete)"
	@echo "k8s-image  - Build workstation image and load into minikube"
	@echo "k8s-verify - Run infra/k8s integration check against local cluster"

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

run-d:
	$(DC) up -d

stop:
	$(DC) stop

down:
	$(DC) down

ps:
	$(DC) ps

log:
	$(DC) logs -f $(s)

reset:
	$(DC) down -v
	$(DC) up -d --build

delete:
	$(DC) down -v --remove-orphans
	docker images --format '{{.Repository}}' | grep '^$(PROJECT_NAME)-' | xargs -r docker rmi -f
	yes | docker network prune --filter name=$(PROJECT_NAME)_ 2>/dev/null; true
	yes | docker volume prune --filter name=$(PROJECT_NAME)_ 2>/dev/null; true

sh:
	$(DC) exec -u $(u) $(s) sh

k8s-up:
	minikube start

k8s-down:
	minikube delete

k8s-image:
	docker build -t aga-workstation:latest -f infra/k8s/workstation-image/Dockerfile infra/k8s/workstation-image
	minikube image load aga-workstation:latest

k8s-verify:
	bash infra/k8s/verify.sh

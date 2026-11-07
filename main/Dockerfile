ARG KUBECTL_VERSION=v1.34.2
# sha256 kubectl ${KUBECTL_VERSION} (dl.k8s.io/release/<ver>/bin/linux/<arch>/kubectl.sha256):
# при смене версии обновить оба.
ARG KUBECTL_SHA256_AMD64=9591f3d75e1581f3f7392e6ad119aab2f28ae7d6c6e083dc5d22469667f27253
ARG KUBECTL_SHA256_ARM64=95df604e914941f3172a93fa8feeb1a1a50f4011dfbe0c01e01b660afc8f9b85

FROM rust:1.98-slim-bookworm AS builder

WORKDIR /app

# Устанавливаем зависимости для сборки
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    curl \
    && rm -rf /var/lib/apt/lists/*

# kubectl для управления кластером из пода (cluster.rs / agent.rs).
# Статический бинарь: версия — ARG, архитектура — платформа сборки
# (TARGETARCH от BuildKit), контрольная сумма сверяется с закреплённой.
ARG KUBECTL_VERSION
ARG KUBECTL_SHA256_AMD64
ARG KUBECTL_SHA256_ARM64
ARG TARGETARCH
RUN set -eu; \
    arch="${TARGETARCH:-amd64}"; \
    case "$arch" in \
      amd64) sum="$KUBECTL_SHA256_AMD64" ;; \
      arm64) sum="$KUBECTL_SHA256_ARM64" ;; \
      *) echo "unsupported arch: $arch" >&2; exit 1 ;; \
    esac; \
    curl -fsSL "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/${arch}/kubectl" -o /tmp/kubectl; \
    echo "${sum}  /tmp/kubectl" | sha256sum -c -; \
    chmod +x /tmp/kubectl

# Зависимости — строго по Cargo.lock (--locked). Реестр cargo и target/ —
# кеш BuildKit между сборками (в слой не попадают), поэтому бинарь копируется
# из target/ в том же RUN. Фингерпринт приложения сбрасываем: зависимости
# берутся из кеша, а сам крейт пересобирается всегда — метки времени COPY не
# гарантируют, что cargo заметит правки исходников.
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
# Промпты агентов встраиваются в бинарь (include_str! в agent.rs).
COPY prompts/ ./prompts/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    rm -rf target/release/.fingerprint/aga-* \
    && cargo build --release --locked \
    && cp target/release/aga /app/aga

# Статический docker-клиент (без демона): воркстейшны в dev-режиме —
# контейнеры (AGA_WS_BACKEND=docker, cluster.rs / agent.rs).
FROM docker:27-cli AS docker-cli

# Финальный образ
FROM debian:bookworm-slim

# Создаём пользователя для запуска
RUN useradd -r -u 1000 -g nogroup aga

WORKDIR /app

# Копируем бинарник из builder
COPY --from=builder /app/aga /app/aga
COPY --from=builder /tmp/kubectl /usr/local/bin/kubectl
COPY --from=docker-cli /usr/local/bin/docker /usr/local/bin/docker

# Веб-клиент — отдельный сервис (front/), ядро статику не раздаёт.

# Создаём директории для данных
RUN mkdir -p /var/lib/aga/work /etc/aga/keys && \
    chown -R aga:nogroup /var/lib/aga /etc/aga && \
    chmod 755 /var/lib/aga /var/lib/aga/work

USER aga

ENTRYPOINT ["/app/aga"]

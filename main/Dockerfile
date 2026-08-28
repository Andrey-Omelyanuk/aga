ARG KUBECTL_VERSION=v1.34.2

FROM rust:1.98-slim-bookworm as builder

WORKDIR /app

# Устанавливаем зависимости для сборки
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    curl \
    && rm -rf /var/lib/apt/lists/*

# kubectl для управления кластером из пода (cluster.rs / agent.rs).
# Статический бинарь, версия фиксируется ARG'ом на этапе сборки.
ARG KUBECTL_VERSION
RUN curl -fsSL "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/amd64/kubectl" \
    -o /tmp/kubectl && chmod +x /tmp/kubectl

# Копируем манифесты зависимостей
COPY Cargo.toml Cargo.lock* ./

# Создаём фейковые исходники для кэширования слоев
RUN mkdir -p src && echo "fn main() {}" > src/main.rs

# Скачиваем зависимости (кэшируется)
RUN cargo fetch || true
RUN cargo build --release || true

# Удаляем фейковый код и копируем реальный
RUN rm -rf src
COPY src/ ./src/

# Собираем релизную версию. Стаб сбрасываем принудительно: метки времени
# COPY и стабового src/main.rs совпадают в пределах секунды, и cargo считает
# реальные исходники «свежими» — без сброса в образ попадал бинарь от
# fn main(){}. Сносим только фингерпринт приложения: зависимости остаются в кэше.
RUN rm -rf target/release/.fingerprint/aga-* && cargo build --release

# Финальный образ
FROM debian:bookworm-slim

# Создаём пользователя для запуска
RUN useradd -r -u 1000 -g nogroup aga

WORKDIR /app

# Копируем бинарник из builder
COPY --from=builder /app/target/release/aga /app/aga
COPY --from=builder /tmp/kubectl /usr/local/bin/kubectl

# Веб-клиент — отдельный сервис (front/), ядро статику не раздаёт.

# Создаём директории для данных
RUN mkdir -p /var/lib/aga/work /etc/aga/keys && \
    chown -R aga:nogroup /var/lib/aga /etc/aga && \
    chmod 755 /var/lib/aga /var/lib/aga/work

USER aga

ENTRYPOINT ["/app/aga"]

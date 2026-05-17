FROM rust:1.75-slim-bookworm as builder

WORKDIR /app

# Устанавливаем зависимости для сборки
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    cmake \
    && rm -rf /var/lib/apt/lists/*

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

# Собираем релизную версию
RUN cargo build --release

# Финальный образ
FROM debian:bookworm-slim

# Создаём пользователя для запуска
RUN useradd -r -u 1000 -g nogroup aga

WORKDIR /app

# Копируем бинарник из builder
COPY --from=builder /app/target/release/aga /app/aga

# Создаём директории для данных
RUN mkdir -p /var/lib/aga/work /etc/aga/keys && \
    chown -R aga:nogroup /var/lib/aga /etc/aga && \
    chmod 755 /var/lib/aga /var/lib/aga/work

USER aga

ENTRYPOINT ["/app/aga"]

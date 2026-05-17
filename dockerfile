FROM rust:1.75-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src
COPY . .
RUN cargo build --release --bin aga

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    openssh-client ca-certificates && rm -rf /var/lib/apt/lists/*
RUN groupadd -g 1000 aga && useradd -u 1000 -g aga -m -s /bin/bash aga
COPY --from=builder /app/target/release/aga /usr/local/bin/aga
RUN chown aga:aga /usr/local/bin/aga
USER aga
CMD ["aga"]
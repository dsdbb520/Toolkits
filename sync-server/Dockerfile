FROM rust:1.82-slim AS builder
WORKDIR /app

# 安装 C 编译器（rusqlite bundled 需要）
RUN apt-get update && apt-get install -y build-essential && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml ./
COPY src ./src

RUN cargo build --release

# ─── 最终镜像（极小）────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sync-server /usr/local/bin/sync-server

VOLUME ["/data"]
EXPOSE 3000

CMD ["sync-server"]

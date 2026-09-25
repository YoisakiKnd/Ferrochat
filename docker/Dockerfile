FROM node:22-bookworm AS web
WORKDIR /src
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
RUN npm run build

FROM rust:1-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef
WORKDIR /src

FROM chef AS planner
COPY backend/ .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
ARG TARGETARCH
RUN case "$TARGETARCH" in \
      arm64) echo aarch64-unknown-linux-musl ;; \
      *) echo x86_64-unknown-linux-musl ;; \
    esac > /tmp/rust-target \
    && rustup target add "$(cat /tmp/rust-target)"
RUN cargo chef cook --release --target "$(cat /tmp/rust-target)" --recipe-path recipe.json
COPY backend/ .
COPY --from=web /src/build /frontend/build
ENV FERROCHAT_FRONTEND_DIR=/frontend/build
RUN cargo build --release --target "$(cat /tmp/rust-target)" --bin ferrochat \
    && cp "/src/target/$(cat /tmp/rust-target)/release/ferrochat" /ferrochat

FROM debian:bookworm-slim AS certs
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

FROM scratch AS slim
COPY --from=certs /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /ferrochat /ferrochat
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt \
    FERROCHAT_DATA_DIR=/data \
    FERROCHAT_HOST=0.0.0.0 \
    FERROCHAT_PORT=8080
VOLUME /data
EXPOSE 8080
HEALTHCHECK CMD ["/ferrochat", "healthcheck"]
ENTRYPOINT ["/ferrochat"]

FROM debian:bookworm-slim AS full
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --create-home ferrochat
COPY --from=builder /ferrochat /usr/local/bin/ferrochat
ENV FERROCHAT_DATA_DIR=/data FERROCHAT_HOST=0.0.0.0 FERROCHAT_PORT=8080
VOLUME /data
EXPOSE 8080
USER ferrochat
HEALTHCHECK CMD ["/usr/local/bin/ferrochat", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/ferrochat"]

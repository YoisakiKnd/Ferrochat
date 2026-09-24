FROM node:22-bookworm AS web
WORKDIR /src
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
RUN npm run build

FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /src

FROM chef AS planner
COPY backend/ .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY backend/ .
COPY --from=web /src/build /frontend/build
ENV FERROCHAT_FRONTEND_DIR=/frontend/build
RUN cargo build --release --bin ferrochat

FROM gcr.io/distroless/cc-debian12:nonroot AS slim
COPY --from=builder /src/target/release/ferrochat /usr/local/bin/ferrochat
ENV FERROCHAT_DATA_DIR=/data FERROCHAT_HOST=0.0.0.0 FERROCHAT_PORT=8080
VOLUME /data
EXPOSE 8080
USER nonroot
HEALTHCHECK CMD ["/usr/local/bin/ferrochat", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/ferrochat"]

FROM debian:bookworm-slim AS full
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh \
    && apt-get clean && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --create-home ferrochat
COPY --from=builder /src/target/release/ferrochat /usr/local/bin/ferrochat
ENV FERROCHAT_DATA_DIR=/data FERROCHAT_HOST=0.0.0.0 FERROCHAT_PORT=8080
VOLUME /data
EXPOSE 8080
USER ferrochat
HEALTHCHECK CMD ["/usr/local/bin/ferrochat", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/ferrochat"]

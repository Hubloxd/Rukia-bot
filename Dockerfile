# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        cmake \
        libopus-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        ffmpeg \
        python3 \
        python3-pip \
        unzip \
    && curl -fsSL https://deno.land/install.sh | DENO_INSTALL=/usr/local sh \
    && pip3 install --no-cache-dir --break-system-packages 'yt-dlp[default]' \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/Rukia-bar /usr/local/bin/rukia-bar

WORKDIR /app

ENTRYPOINT ["/usr/local/bin/rukia-bar"]

# syntax=docker/dockerfile:1.7
# journent — multi-stage build. Rust compiles in-container, minimal runtime.

################################################################################
# Stage 1: build
################################################################################
FROM rust:1-bookworm AS builder

# Dev deps: libpq (postgres client), ca-certs
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libpq-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Layer cache: create dummy src first so cargo fetch caches dependencies independently.
COPY Cargo.toml ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs
# Fetch crates once so subsequent rebuilds only change when src/ changes.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo fetch || true

# Copy real source + manifest.
COPY Cargo.toml ./
COPY src/ ./src/
COPY migrations/ ./migrations/
COPY templates/ ./templates/
COPY static/ ./static/
COPY AGENT_ONBOARDING.md ./AGENT_ONBOARDING.md
COPY skill/ ./skill/
COPY build.rs ./build.rs

# Build release
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp /app/target/release/journent /usr/local/bin/journent

################################################################################
# Stage 2: runtime
################################################################################
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libpq5 \
    tini \
    wget \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Binary
COPY --from=builder /usr/local/bin/journent /usr/local/bin/journent

# Onboarding + frontend assets (embedded via include_dir at build time).
COPY --from=builder /app/AGENT_ONBOARDING.md /app/AGENT_ONBOARDING.md
COPY --from=builder /app/templates/ /app/templates/
COPY --from=builder /app/static/ /app/static/
COPY --from=builder /app/skill/ /app/skill/

# Entrypoint
ENV JOURNENT_BIND=0.0.0.0:8080 \
    JOURNENT_DB_URL=postgres://journent:journent@db:5432/journent \
    RUST_LOG=journent=info,tower_http=info,sqlx=warn

EXPOSE 8080
VOLUME ["/data"]

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["journent"]

# syntax=docker/dockerfile:1

# ---- Build stage ----
# Static musl build -> no libc in the runtime image. Works because the crate
# uses rustls (no OpenSSL) and sqlx bundles SQLite, so there are no C deps.
# edition = "2024" needs Rust >= 1.85; a transitive dep (time) needs >= 1.88.
FROM rust:1.88-alpine AS builder

# musl-dev + the musl target for a fully static binary.
RUN apk add --no-cache musl-dev
WORKDIR /app
ENV TARGET=x86_64-unknown-linux-musl

# Cache dependencies: build against the manifests with a dummy main first, so
# a change under src/ rebuilds only this crate, not every dependency.
COPY Cargo.toml Cargo.lock ./
RUN rustup target add "$TARGET" \
    && mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --target "$TARGET" \
    && rm -rf src

COPY src ./src
COPY templates ./templates
RUN touch src/main.rs && cargo build --release --target "$TARGET" \
    && cp "target/$TARGET/release/calibre-server" /calibre-server

# ---- Runtime stage ----
# scratch = empty image; the static binary is the whole image (~5-15 MB).
FROM scratch AS runtime

# Static binary needs nothing else; ship ca-certificates only so an https
# BASE_URL can validate TLS. Pulled from the alpine builder's bundle.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

COPY --from=builder /calibre-server /calibre-server

# In a container we must bind all interfaces; the library mounts at /library.
ENV HOST=0.0.0.0 \
    PORT=8080 \
    CALIBRE_LIBRARY_PATH=/library

EXPOSE 8080
USER 10001

ENTRYPOINT ["/calibre-server"]

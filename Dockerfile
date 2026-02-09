FROM rust:1.83-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates tzdata wget
RUN addgroup -g 1000 sptzx && adduser -D -u 1000 -G sptzx sptzx
WORKDIR /app
COPY --from=builder /build/target/release/sptzx /app/sptzx
RUN mkdir -p /app/uploads && chown -R sptzx:sptzx /app
USER sptzx

ENV RUST_LOG=info \
    SPTZX_PORT=3000 \
    SPTZX_BIND_ADDR=0.0.0.0:3000 \
    SPTZX_SECRET_KEY="" \
    SPTZX_UPLOAD_DIR=/app/uploads \
    SPTZX_MAX_FILE_SIZE=536870912 \
    SPTZX_FILE_LIFETIME=300 \
    SPTZX_BASE_URL=""

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:${SPTZX_PORT}/ || exit 1

CMD ["/app/sptzx"]

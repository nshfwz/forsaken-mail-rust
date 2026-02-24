FROM rust:1.93-alpine AS builder

WORKDIR /app

RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

COPY . .
RUN cargo build --release

FROM alpine:3.22

WORKDIR /app
RUN apk add --no-cache ca-certificates tzdata

COPY --from=builder /app/target/release/forsaken-mail-rust /usr/local/bin/forsaken-mail-rust

EXPOSE 25 3000

ENTRYPOINT ["/usr/local/bin/forsaken-mail-rust"]

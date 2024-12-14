# Fase de build com Rust
FROM rust:latest AS builder
WORKDIR /usr/src/axum_rinha
COPY . .
RUN cargo install --path .

FROM archlinux:latest
COPY --from=builder /usr/local/cargo/bin/axum_rinha /usr/local/bin/axum_rinha
CMD ["axum_rinha"]

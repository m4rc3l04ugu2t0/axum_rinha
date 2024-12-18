FROM rust:latest AS builder
WORKDIR /usr/src/load_balance
COPY ./load_balance .
RUN cargo install --path .

FROM archlinux:latest
COPY --from=builder /usr/local/cargo/bin/load_balance /usr/local/bin/load_balance
CMD ["load_balance"]

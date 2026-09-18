FROM rust:1.98-bookworm as builder

WORKDIR /usr/src/listmonk-resend

COPY . .

RUN cargo test && \
    cargo install --path .

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/listmonk-resend /usr/local/bin/listmonk-resend

ENV PORT=9000
ENV HOST=0.0.0.0

EXPOSE 9000
ENTRYPOINT ["listmonk-resend"]

FROM rust:1.87 AS builder
WORKDIR /usr/src/app

COPY Cargo.* ./
COPY server/Cargo.* server/
COPY shared/Cargo.* shared/
COPY shared/src shared/src
COPY client/Cargo.* client/
COPY client/src client/src

RUN cargo fetch
RUN cargo build --release -p server --bin server || true

COPY . .
RUN cargo build --release -p server

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

# copy the statically-linked server binary from the previous stage
COPY --from=builder /usr/src/app/target/release/server /usr/local/bin/server

ENV RUST_LOG=info
EXPOSE 3000

CMD ["server"]
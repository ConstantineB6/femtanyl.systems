set dotenv-load
set shell := ["bash", "-cu"]

dev:
  just client &
  just server &
  wait

client:
  cd client && trunk serve

server:
  cd server && cargo watch -p server -x run

fmt:
  cargo fmt --all


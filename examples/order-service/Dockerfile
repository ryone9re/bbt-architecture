FROM rust:1.98.1-bookworm AS build
WORKDIR /app
COPY . .
RUN cargo build --locked --release --workspace --bins

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/server /app/target/release/cleanup /app/target/release/rebuild-sales /app/target/release/migrate /usr/local/bin/
USER 65532:65532
ENV HTTP_ADDR=0.0.0.0:3000
EXPOSE 3000
CMD ["server"]

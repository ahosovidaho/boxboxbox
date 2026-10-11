# BOXBOXBOX – telemetry dashboard and race strategy for GT7
# Multi-stage build: compile in the Rust image, run in a small Debian image.

FROM rust:1-bookworm AS build
WORKDIR /src
# Dependencies first, so code changes don't re-download/re-build all crates
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src target/release/deps/boxboxbox* target/release/boxboxbox*
COPY src ./src
COPY web ./web
COPY data ./data
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 1000 --home-dir /data boxbox \
    && mkdir -p /data && chown boxbox /data
COPY --from=build /src/target/release/boxboxbox /usr/local/bin/boxboxbox
USER boxbox
VOLUME /data
# 8080/tcp dashboard, 33740/udp telemetry from the PS5
EXPOSE 8080/tcp 33740/udp
ENTRYPOINT ["boxboxbox", "--config", "/data/config.toml", "--data-dir", "/data"]

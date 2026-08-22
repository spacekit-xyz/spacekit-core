FROM rust:1.91-bookworm AS builder

WORKDIR /workspace
COPY routekit/Cargo.toml routekit/Cargo.toml
COPY routekit/src routekit/src
COPY spacekit-primitives/Cargo.toml spacekit-primitives/Cargo.toml
COPY spacekit-primitives/src spacekit-primitives/src
COPY spacekit-primitives/vendor/pqcrypto-internals spacekit-primitives/vendor/pqcrypto-internals
COPY spacekit-primitives/vendor/pqcrypto-kyber spacekit-primitives/vendor/pqcrypto-kyber
RUN printf '[workspace]\nresolver = "2"\nmembers = ["routekit", "spacekit-primitives"]\n\
[patch.crates-io]\npqcrypto-internals = { path = "spacekit-primitives/vendor/pqcrypto-internals" }\n\
pqcrypto-kyber = { path = "spacekit-primitives/vendor/pqcrypto-kyber" }\n' > Cargo.toml
RUN cargo generate-lockfile \
    && cargo build --locked --release -p routekit --no-default-features --features complete

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 routekit

COPY --from=builder /workspace/target/release/routekit /usr/local/bin/routekit

USER routekit
EXPOSE 3001

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
  CMD curl --fail --silent http://127.0.0.1:3001/health >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/routekit"]

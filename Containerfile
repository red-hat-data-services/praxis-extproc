# Multi-stage build for praxis-extproc.
#
# Builder: ubi9/ubi with AppStream rust-toolset (1.92) and openssl-devel.
# Cargo --ignore-rust-version: crates declare rust-version 1.96; rustc 1.92
# type-checks this tree. OPENSSL_NO_VENDOR forces openssl-sys onto system
# libssl.so.3 (UBI9 OpenSSL) instead of a vendored copy.
# Runtime: ubi9/ubi-minimal + openssl-libs (same SONAME).
#
# Build:
#   make container-release
#
# Run:
#   docker run -p 50051:50051 -p 50052:50052 -p 9090:9090 \
#     -v $(pwd)/examples/praxis-extproc.yaml:/etc/praxis/extproc.yaml \
#     praxis-extproc:dev -c /etc/praxis/extproc.yaml

# ---------------------------------------------------------------------------
# Builder
# ---------------------------------------------------------------------------

FROM registry.access.redhat.com/ubi9/ubi AS builder

ARG CARGO_PROFILE=release

RUN dnf install -y rust-toolset openssl-devel gcc gcc-c++ cmake make \
    && dnf clean all

ENV OPENSSL_NO_VENDOR=1

WORKDIR /build
COPY . .


RUN set -eu; \
    if [ "${CARGO_PROFILE}" = "release" ]; then \
      cargo build --ignore-rust-version --release --bin praxis-extproc; \
      BIN=target/release/praxis-extproc; \
      strip "${BIN}"; \
    elif [ "${CARGO_PROFILE}" = "debug" ]; then \
      cargo build --ignore-rust-version --bin praxis-extproc; \
      BIN=target/debug/praxis-extproc; \
    else \
      echo "unsupported CARGO_PROFILE=${CARGO_PROFILE}" >&2; \
      exit 1; \
    fi; \
    cp "${BIN}" /praxis-extproc

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------

FROM registry.access.redhat.com/ubi9/ubi-minimal

RUN microdnf install -y openssl-libs \
    && microdnf clean all

COPY --from=builder /praxis-extproc /usr/local/bin/praxis-extproc

USER 1001

EXPOSE 50051 50052 9090

ENTRYPOINT ["praxis-extproc"]
CMD ["-c", "/etc/praxis/extproc.yaml"]

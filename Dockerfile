# Toolchain bump: edit rust-toolchain.toml only; this image reads the channel
# from it. Bump RUSTUP_VERSION (plus its checksums) separately.
FROM alpine:3.24.2@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6

ARG TARGETARCH
ARG RUSTUP_VERSION=1.29.1
ARG RUSTUP_X86_64_SHA256=331228566cc931f32cd684f9bebc5956a5e5d7394c8d5de595f196a658ab98e6
ARG RUSTUP_AARCH64_SHA256=1ddf36182ac5d1782dbeefcb9bea7f5f9412d88f4a0d5047b1564a223624e8c6

RUN apk add --no-cache \
    binutils=2.45.1-r1 \
    bash=5.3.9-r1 \
    file=5.47-r2 \
    make=4.4.1-r4 \
    gcc=15.2.0-r5 \
    musl-dev=1.2.6-r3

RUN case "$TARGETARCH" in \
        amd64) arch=x86_64; sum=$RUSTUP_X86_64_SHA256 ;; \
        arm64) arch=aarch64; sum=$RUSTUP_AARCH64_SHA256 ;; \
        *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && init=/tmp/rustup-init \
    && wget -q -O "$init" "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/${arch}-unknown-linux-musl/rustup-init" \
    && echo "$sum  $init" | sha256sum -c - \
    && chmod +x "$init" \
    && "$init" -y --default-toolchain none \
    && rm "$init"

ENV PATH="/root/.cargo/bin:$PATH"

COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' /tmp/rust-toolchain.toml)" \
    && test -n "$channel" \
    && rustup toolchain install "$channel" \
        --target x86_64-unknown-linux-musl \
        --target aarch64-unknown-linux-musl \
        --component rust-src \
        --component clippy \
    && rustup default "$channel" \
    && rm /tmp/rust-toolchain.toml

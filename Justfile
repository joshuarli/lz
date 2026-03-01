target := arch() + "-apple-darwin"

build:
    cargo build

release:
    cargo clean -p lz --release --target {{ target }}
    RUSTFLAGS="-Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort" \
    cargo +nightly build --release \
      -Z build-std=std \
      -Z build-std-features= \
      --target {{ target }}

install: release
    cp target/{{ target }}/release/lz ~/usr/bin

setup:
  prek install --install-hooks

pc:
  prek run --all-files

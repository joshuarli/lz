NAME   := lz
HOST   := $(shell rustc -vV | awk '/^host:/ {print $$2}')
TARGET ?= $(subst -unknown-linux-gnu,-unknown-linux-musl,$(HOST))
TARGET_ENV := $(shell printf '%s' "$(TARGET)" | tr '[:lower:]-' '[:upper:]_')
CARGO_CMD  := $(if $(findstring -linux-musl,$(TARGET)),$(shell command -v musl-cargo 2>/dev/null || printf cargo),cargo)
cargo = $(if $(findstring -linux-musl,$(TARGET)),,$(if $(filter test,$(1)),CARGO_TARGET_$(TARGET_ENV)_LINKER=clang ,)) MUSL_TARGET="$(TARGET)" MUSL_BUILD_MODE="$(1)" $(CARGO_CMD)

RUSTYBENCH_DEFAULT := $(CURDIR)/.work/tools/bin/rustybench
RUSTYBENCH ?= "$(RUSTYBENCH_DEFAULT)"

.PHONY: setup build release install test test-ci pc bump-version lint verify-release bench bench-syscalls rustybench

setup:
	rustup show active-toolchain
	prek install --install-hooks

build:
	$(call cargo,dev) build

release:
	$(call cargo,release-static) clean -p $(NAME) --profile dist --target $(TARGET)
	RUSTFLAGS="-Zlocation-detail=none -Zunstable-options -Cpanic=immediate-abort" \
	$(call cargo,release-static) build --profile dist -p $(NAME) \
	  -Z build-std=std \
	  -Z build-std-features= \
	  --target $(TARGET)

test:
	$(call cargo,test) test --quiet

test-ci:
	@test -x "target/$(TARGET)/dist/$(NAME)" || { echo 'run make release first' >&2; exit 1; }
	$(call cargo,test) test --quiet --release

verify-release:
	@test -f "target/$(TARGET)/dist/$(NAME)"
	@if echo "$(TARGET)" | grep -q -- '-linux-musl$$'; then \
		command -v readelf >/dev/null || { echo 'readelf is required for release verification'; exit 1; }; \
		file "target/$(TARGET)/dist/$(NAME)" | grep -Eq 'static-pie linked|statically linked' || { echo 'release is not statically linked'; exit 1; }; \
		file "target/$(TARGET)/dist/$(NAME)" | grep -q 'stripped' || { echo 'release is not stripped'; exit 1; }; \
		! readelf -l "target/$(TARGET)/dist/$(NAME)" | grep -q INTERP || { echo 'release has a dynamic ELF interpreter'; exit 1; }; \
		! readelf -d "target/$(TARGET)/dist/$(NAME)" | grep -q NEEDED || { echo 'release has dynamic dependencies'; exit 1; }; \
	else \
		echo "Skipping ELF checks for $(TARGET)"; \
	fi

install: release
	@set -eu; \
	  install_dir="$(HOME)/usr/bin"; \
	  mkdir -p "$$install_dir"; \
	  install -m 755 "target/$(TARGET)/dist/$(NAME)" "$$install_dir/$(NAME)"; \
	  if test "$$(uname -s)" = Darwin; then codesign -fs - "$$install_dir/$(NAME)"; fi

pc:
	prek run --quiet --all-files

lint:
	cargo clippy --workspace --all-targets -- -D warnings

ifeq ($(RUSTYBENCH),"$(RUSTYBENCH_DEFAULT)")
bench bench-syscalls: rustybench
endif

rustybench:
	@mkdir -p "$(CURDIR)/.work/tmp" "$(CURDIR)/.work/tools"
	@if test ! -x "$(RUSTYBENCH_DEFAULT)"; then TMPDIR="$(CURDIR)/.work/tmp" cargo install rustybench --version 0.3.0 --locked --root "$(CURDIR)/.work/tools"; fi

bench:
	@TMPDIR="$(CURDIR)/.work/tmp" $(RUSTYBENCH) baseline --root "$(CURDIR)" --baseline "$(CURDIR)/benches/baseline.json" -- cargo bench --bench bench

bench-syscalls:
	@TMPDIR="$(CURDIR)/.work/tmp" $(RUSTYBENCH) syscalls --root "$(CURDIR)"

# Usage: make bump-version [V=x.y.z]
# Without V, increments the patch version.
bump-version:
ifndef V
	$(eval OLD := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml))
	$(eval V := $(shell echo "$(OLD)" | awk -F. '{printf "%d.%d.%d", $$1, $$2, $$3+1}'))
endif
	sed -i '' 's/^version = ".*"/version = "$(V)"/' Cargo.toml
	cargo check --quiet 2>/dev/null
	git add Cargo.toml Cargo.lock
	git commit -m "bump version to $(V)"

.PHONY: parity-setup parity-test
parity-setup:
	@mkdir -p "$(CURDIR)/.work/tmp"
	TMPDIR="$(CURDIR)/.work/tmp" python3 tests/parity/setup.py
	TMPDIR="$(CURDIR)/.work/tmp" cargo fetch --locked

parity-test:
	@TMPDIR="$(CURDIR)/.work/tmp" python3 tests/parity/setup.py --verify >/dev/null
	TMPDIR="$(CURDIR)/.work/tmp" CARGO_NET_OFFLINE=true cargo test --test parity -- $(if $(CASE),--case $(CASE),) $(if $(GROUP),--group $(GROUP),) --mode $(if $(MODE),$(MODE),admitted)

.PHONY: parity-selftest
parity-selftest:
	TMPDIR="$(CURDIR)/.work/tmp" CARGO_NET_OFFLINE=true cargo test --test parity -- --selftest

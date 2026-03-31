.PHONY: build release test clean install all

# Default target
all: build

# Development build
build:
	cargo build

# Release build (current platform)
release:
	cargo build --release

# Run all tests
test:
	cargo test --workspace

# Run adapter tests
test-adapters:
	./scripts/test-all-commands.sh

# Compare with opencli
test-compare:
	./scripts/test-all-commands.sh both

# Clean build artifacts
clean:
	cargo clean

# Install to /usr/local/bin
install: release
	cp target/release/opencli-rs /usr/local/bin/opencli-rs
	@echo "✓ Installed to /usr/local/bin/opencli-rs"

# Uninstall
uninstall:
	rm -f /usr/local/bin/opencli-rs
	@echo "✓ Removed /usr/local/bin/opencli-rs"

# ── Cross-compilation targets ──

release-mac-arm:
	cargo build --release --target aarch64-apple-darwin

release-mac-intel:
	cargo build --release --target x86_64-apple-darwin

release-linux:
	cargo build --release --target x86_64-unknown-linux-musl

release-linux-arm:
	cross build --release --target aarch64-unknown-linux-musl

release-windows:
	cargo build --release --target x86_64-pc-windows-msvc

# Build all platforms (requires cross for ARM Linux)
release-all: release-mac-arm release-mac-intel release-linux release-linux-arm release-windows

# ── Packaging ──

TARGETS = aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-pc-windows-msvc

package:
	@mkdir -p dist
	@for target in $(TARGETS); do \
		if [ -f "target/$$target/release/opencli-rs" ]; then \
			echo "Packaging $$target..."; \
			tar czf "dist/opencli-rs-$$target.tar.gz" -C "target/$$target/release" opencli-rs; \
		elif [ -f "target/$$target/release/opencli-rs.exe" ]; then \
			echo "Packaging $$target..."; \
			cd "target/$$target/release" && zip "../../../dist/opencli-rs-$$target.zip" opencli-rs.exe && cd ../../..; \
		fi \
	done
	@cd dist && sha256sum opencli-rs-* > SHA256SUMS.txt 2>/dev/null || shasum -a 256 opencli-rs-* > SHA256SUMS.txt
	@echo "✓ Packages in dist/"
	@ls -lh dist/

# ── Info ──

info:
	@echo "opencli-rs v$$(cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin)['packages'][0]['version'])" 2>/dev/null || echo 'unknown')"
	@echo ""
	@echo "Targets:"
	@echo "  make build          - Debug build"
	@echo "  make release        - Release build (current platform)"
	@echo "  make test           - Run unit tests"
	@echo "  make test-adapters  - Run adapter integration tests"
	@echo "  make test-compare   - Compare opencli-rs vs opencli"
	@echo "  make install        - Install to /usr/local/bin"
	@echo "  make release-all    - Cross-compile all platforms"
	@echo "  make package        - Create distributable archives"

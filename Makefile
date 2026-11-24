CARGO?=cargo
PREFIX?=/usr/local
DESTDIR?=
INSTALL_ARGS?=
UNINSTALL_ARGS?=

.PHONY: all build release test check fmt clippy install install-auto install-source install-prebuilt install-check uninstall uninstall-check stage stage-auto stage-prebuilt clean

all: build

build:
	$(CARGO) build --workspace

release:
	$(CARGO) build --release --workspace

test:
	$(CARGO) test --workspace

fmt:
	$(CARGO) fmt --all

check:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

# Auto: build when the toolchain is available, otherwise use prebuilt artifacts.
install: install-auto

install-auto:
	./scripts/install-freebsd.sh --auto $(INSTALL_ARGS)

install-source:
	./scripts/install-freebsd.sh --source $(INSTALL_ARGS)

install-prebuilt:
	./scripts/install-freebsd-prebuilt.sh $(INSTALL_ARGS)

install-check:
	./scripts/install-freebsd.sh --check

stage: stage-auto

stage-auto:
	DESTDIR=$(DESTDIR) PREFIX=$(PREFIX) ./scripts/install-freebsd.sh --auto $(INSTALL_ARGS)

stage-prebuilt:
	DESTDIR=$(DESTDIR) PREFIX=$(PREFIX) ./scripts/install-freebsd-prebuilt.sh $(INSTALL_ARGS)

# Destructive: run as root. Use UNINSTALL_ARGS=--yes for unattended uninstall.
uninstall:
	DESTDIR=$(DESTDIR) PREFIX=$(PREFIX) ./scripts/uninstall-freebsd.sh $(UNINSTALL_ARGS)

uninstall-check:
	DESTDIR=$(DESTDIR) PREFIX=$(PREFIX) ./scripts/uninstall-freebsd.sh --dry-run

clean:
	$(CARGO) clean
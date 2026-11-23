CARGO?=cargo
PREFIX?=/usr/local
DESTDIR?=

.PHONY: all build release test check fmt clippy install stage clean

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

install:
	./scripts/install-freebsd.sh

stage:
	DESTDIR=$(DESTDIR) PREFIX=$(PREFIX) ./scripts/install-freebsd.sh

clean:
	$(CARGO) clean

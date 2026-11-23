# SPDX-FileCopyrightText: 2026 netcore contributors
# SPDX-License-Identifier: BSD-2-Clause

PREFIX ?= /usr/local
DESTDIR ?=
BUILD_DIR ?= build

.PHONY: all build check install clean

all: build

build:
	./build.sh --build

check:
	./build.sh --check

install:
	PREFIX="$(PREFIX)" DESTDIR="$(DESTDIR)" BUILD_DIR="$(BUILD_DIR)" ./build.sh --install

clean:
	./build.sh --clean

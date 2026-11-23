#!/bin/sh
set -eu

# Direct installation of already-built artifacts. Any conflicting mode flag is
# rejected by the main installer, so this wrapper can never start a build.
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$SCRIPT_DIR/install-freebsd.sh" "$@" --binary

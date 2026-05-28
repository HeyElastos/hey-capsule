#!/usr/bin/env bash
#
# Deploy hey-social into the locally-installed Elastos Runtime
# WITHOUT pushing to GitHub. Mirrors what scripts/update-hey-only.sh in
# elastos-runtime_ynh does for the Rust capsule, but reads from the
# local checkout instead of fetching from the HeyElastos/Hey-capsule
# tarball — so you can iterate + test BEFORE committing or pushing.
#
# Usage (must be root, this writes under /home/yunohost.app/...):
#
#   sudo bash capsules/hey-social/scripts/deploy-local.sh
#
# What it does:
#   1. Builds the WASM bundle with `trunk build` (needs trunk on PATH).
#   2. Copies the capsule directory (including dist/) into the runtime's
#      data_dir under /home/yunohost.app/elastos_runtime/.../capsules/.
#   3. chowns to the runtime user.
#   4. Restarts the elastos_runtime systemd service.
#   5. Prints the URL to hit in the browser.

set -euo pipefail

CAPSULE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_USER="elastos_runtime"
DATA_DIR="/home/yunohost.app/elastos_runtime/home/xdg-data/elastos/capsules"
DOMAIN_PATH="https://elastos.app/elastos/apps/hey-social/"

if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: must run as root (use sudo)" >&2
    exit 1
fi
if [ ! -d "$DATA_DIR" ]; then
    echo "ERROR: $DATA_DIR does not exist. Is elastos_runtime installed?" >&2
    exit 1
fi

# Resolve trunk on the invoking user's PATH if root doesn't have it.
TRUNK_BIN="$(command -v trunk || true)"
if [ -z "$TRUNK_BIN" ] && [ -n "${SUDO_USER:-}" ]; then
    TRUNK_BIN="$(sudo -u "$SUDO_USER" bash -lc 'command -v trunk' || true)"
fi
if [ -z "$TRUNK_BIN" ]; then
    echo "ERROR: trunk not found on PATH" >&2
    exit 1
fi

echo "=== Building $CAPSULE_DIR with $TRUNK_BIN ==="
if [ -n "${SUDO_USER:-}" ]; then
    sudo -u "$SUDO_USER" bash -lc "cd '$CAPSULE_DIR' && '$TRUNK_BIN' build"
else
    (cd "$CAPSULE_DIR" && "$TRUNK_BIN" build)
fi

if [ ! -f "$CAPSULE_DIR/dist/index.html" ]; then
    echo "ERROR: trunk build produced no dist/index.html" >&2
    exit 1
fi

echo "=== Deploying to $DATA_DIR/hey-social ==="
rm -rf "$DATA_DIR/hey-social"
cp -r "$CAPSULE_DIR" "$DATA_DIR/hey-social"
# Strip dev cruft that doesn't belong in the deployed capsule.
rm -rf "$DATA_DIR/hey-social/scripts" \
       "$DATA_DIR/hey-social/target" \
       "$DATA_DIR/hey-social/Cargo.lock" \
       "$DATA_DIR/hey-social/.cargo"
chown -R "$APP_USER:$APP_USER" "$DATA_DIR/hey-social"

echo "=== Restarting elastos_runtime ==="
systemctl restart elastos_runtime

echo
echo "=== Done. Hard-refresh the browser, then visit: ==="
echo "  $DOMAIN_PATH"
echo
echo "If sign-in fails, check the browser console for [hey-social]"
echo "diagnostic logs from the /authenticate/begin + /complete calls."

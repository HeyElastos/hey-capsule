#!/usr/bin/env bash
#
# Deploy hey-social from THIS dev machine to a remote YunoHost
# server running elastos_runtime, WITHOUT pushing to GitHub. Builds
# locally with trunk, rsyncs the capsule directory to the server, then
# SSH's in to drop it into data_dir + restart the service.
#
# Usage (run on your dev machine):
#
#   ./capsules/hey-social/scripts/deploy-remote.sh
#   REMOTE=pc2@jothgard ./capsules/hey-social/scripts/deploy-remote.sh
#
# Defaults to pc2@jothgard. The remote user must have sudo (will be
# prompted once for the password during the install step).

set -euo pipefail

REMOTE="${REMOTE:-pc2@jothgard}"
CAPSULE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REMOTE_STAGING="/tmp/hey-social-deploy"
REMOTE_DATA_DIR="/home/yunohost.app/elastos_runtime/home/xdg-data/elastos/capsules"
APP_USER="elastos_runtime"
DOMAIN_PATH="https://elastos.app/elastos/apps/hey-social/"

echo "=== [1/4] Building $CAPSULE_DIR with trunk ==="
(cd "$CAPSULE_DIR" && trunk build)
if [ ! -f "$CAPSULE_DIR/dist/index.html" ]; then
    echo "ERROR: trunk build produced no dist/index.html" >&2
    exit 1
fi

echo "=== [2/4] Rsyncing capsule to $REMOTE:$REMOTE_STAGING ==="
rsync -avz --delete \
    --exclude target \
    --exclude scripts \
    --exclude Cargo.lock \
    --exclude .cargo \
    "$CAPSULE_DIR/" "$REMOTE:$REMOTE_STAGING/"

echo "=== [3/4] Installing into $REMOTE:$REMOTE_DATA_DIR/hey-social ==="
ssh -t "$REMOTE" "sudo bash -s" <<EOF
set -euo pipefail
if [ ! -d "$REMOTE_DATA_DIR" ]; then
    echo "ERROR: $REMOTE_DATA_DIR does not exist on remote — is elastos_runtime installed?" >&2
    exit 1
fi
rm -rf "$REMOTE_DATA_DIR/hey-social"
cp -r "$REMOTE_STAGING" "$REMOTE_DATA_DIR/hey-social"
chown -R "$APP_USER:$APP_USER" "$REMOTE_DATA_DIR/hey-social"
systemctl restart elastos_runtime
EOF

echo "=== [4/4] Done. Hard-refresh the browser, then visit: ==="
echo "  $DOMAIN_PATH"
echo
echo "If sign-in fails, check the browser console for [hey-social]"
echo "diagnostic logs from the /authenticate/begin + /complete calls."

#!/bin/bash
set -euo pipefail

# jetrun deployment script
# Run as root: sudo bash deploy/install.sh

INSTALL_DIR="/opt/jetrun"
BIN_DIR="$INSTALL_DIR/bin"
DATA_DIR="$INSTALL_DIR/data"
WEB_DIR="$INSTALL_DIR/web"
SERVICE_USER="ubuntu"

echo "=== jetrun deployment ==="

# 1. Create directories
echo "[1/6] Creating directories..."
mkdir -p "$BIN_DIR" "$DATA_DIR"/{cache,workspace,artifacts} "$WEB_DIR"
chown -R "$SERVICE_USER:$SERVICE_USER" "$INSTALL_DIR"

# 2. Copy binaries
echo "[2/6] Installing binaries..."
for bin in jetrun-gateway jetrun-engine jetrun-worker jetrun-cache jetrun-auth; do
    if [ -f "target/release/$bin" ]; then
        cp "target/release/$bin" "$BIN_DIR/$bin"
        chmod 755 "$BIN_DIR/$bin"
        echo "  Installed $bin"
    else
        echo "  WARNING: target/release/$bin not found — build with: cargo build --release"
    fi
done

# 3. Build and copy frontend
echo "[3/6] Installing frontend..."
if [ -d "web/.next/standalone" ]; then
    cp -r web/.next/standalone/* "$WEB_DIR/"
    cp -r web/.next/static "$WEB_DIR/.next/static" 2>/dev/null || true
    cp -r web/public "$WEB_DIR/public" 2>/dev/null || true
    chown -R "$SERVICE_USER:$SERVICE_USER" "$WEB_DIR"
    echo "  Installed web dashboard"
else
    echo "  WARNING: web/.next/standalone not found — build with: cd web && npm run build"
fi

# 4. Install systemd units
echo "[4/6] Installing systemd services..."
cp deploy/jetrun-gateway.service /etc/systemd/system/
cp deploy/jetrun-engine.service /etc/systemd/system/
cp deploy/jetrun-worker.service /etc/systemd/system/
cp deploy/jetrun-cache.service /etc/systemd/system/
cp deploy/jetrun-auth.service /etc/systemd/system/
cp deploy/jetrun-web.service /etc/systemd/system/
cp deploy/jetrun.target /etc/systemd/system/
systemctl daemon-reload
echo "  Services installed"

# 5. Enable services
echo "[5/6] Enabling services..."
systemctl enable jetrun-gateway jetrun-engine jetrun-worker jetrun-cache jetrun-auth jetrun-web jetrun.target

# 6. Start everything
echo "[6/6] Starting jetrun..."
systemctl start jetrun.target
sleep 2

echo ""
echo "=== jetrun deployed ==="
echo ""
echo "Services:"
for svc in jetrun-gateway jetrun-engine jetrun-worker jetrun-cache jetrun-auth jetrun-web; do
    status=$(systemctl is-active "$svc" 2>/dev/null || echo "inactive")
    printf "  %-20s %s\n" "$svc" "$status"
done
echo ""
echo "Ports:"
echo "  Gateway API:  http://0.0.0.0:8080"
echo "  Engine:       http://0.0.0.0:9001"
echo "  Worker:       http://0.0.0.0:9002"
echo "  Cache:        http://0.0.0.0:9003"
echo "  Auth:         http://0.0.0.0:9004"
echo "  Web Dashboard: http://0.0.0.0:3000"
echo ""
echo "Logs:  journalctl -u jetrun-gateway -f"
echo "All:   journalctl -u 'jetrun-*' -f"
echo ""
echo "IMPORTANT: Edit /etc/systemd/system/jetrun-*.service to set:"
echo "  - JWT_SECRET (must match between gateway and auth)"
echo "  - SUPERADMIN_PASSWORD"
echo "  - WEBHOOK_SECRET (for GitHub/GitLab webhooks)"
echo "Then: sudo systemctl daemon-reload && sudo systemctl restart jetrun.target"

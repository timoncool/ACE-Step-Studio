#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "========================================"
echo "  ACE-Step Studio - Clean Reinstall"
echo "========================================"
echo ""
echo " This will DELETE and reinstall:"
echo "   - venv/             (Python + all packages)"
echo "   - node/             (Node.js runtime)"
echo "   - app/node_modules  (npm packages)"
echo "   - app/server/node_modules"
echo "   - app/dist          (frontend build)"
echo "   - downloads/        (cached installers)"
echo ""
echo " This will KEEP (safe):"
echo "   - models/           (downloaded models)"
echo "   - output/           (generated audio)"
echo "   - cache/            (HF cache with models)"
echo "   - app/data/         (database, settings)"
echo "   - app/server/public/audio/ (saved songs)"
echo "   - datasets/         (training data)"
echo "   - ffmpeg/           (ffmpeg binary)"
echo "   - lora_output/      (trained LoRAs)"
echo "   - ACE-Step-1.5/     (source code, not deps)"
echo "   - cuda_version.txt  (GPU config)"
echo ""

read -rp "Type YES to continue: " CONFIRM
if [ "$CONFIRM" != "YES" ]; then
    echo "Cancelled."
    exit 0
fi

echo ""
echo "============================================================"
echo " Cleaning software directories..."
echo "============================================================"

echo "IMPORTANT: Close ACE-Step Studio (run.sh) before continuing!"
echo "If the app is running, files may be locked and cleanup will fail."
echo ""

if [ -d "venv" ]; then
    echo "Removing venv/..."
    rm -rf venv
    echo "  [OK] venv removed"
fi

if [ -d "node" ]; then
    echo "Removing node/..."
    rm -rf node
    echo "  [OK] node removed"
fi

if [ -d "app/node_modules" ]; then
    echo "Removing app/node_modules/..."
    rm -rf app/node_modules
    echo "  [OK] app/node_modules removed"
fi

if [ -d "app/server/node_modules" ]; then
    echo "Removing app/server/node_modules/..."
    rm -rf app/server/node_modules
    echo "  [OK] app/server/node_modules removed"
fi

if [ -d "app/dist" ]; then
    echo "Removing app/dist/..."
    rm -rf app/dist
    echo "  [OK] app/dist removed"
fi

if [ -d "downloads" ]; then
    echo "Removing downloads/..."
    rm -rf downloads
    echo "  [OK] downloads removed"
fi

[ -f "app/package-lock.json" ] && rm -f app/package-lock.json
[ -f "app/server/package-lock.json" ] && rm -f app/server/package-lock.json

# ============================================================
#  Pull latest code before installing
# ============================================================
if command -v git &>/dev/null && [ -d ".git" ]; then
    echo ""
    echo "Pulling latest code..."
    git stash 2>/dev/null || true
    git pull
    git stash pop 2>/dev/null || true
    echo "  [OK] Code updated"
fi

if [ -f "cuda_version.txt" ]; then
    SAVED_CUDA="$(cat cuda_version.txt)"
    echo ""
    echo "NOTE: Your previous GPU config was: $SAVED_CUDA"
    echo "      install.sh will ask you to select GPU again."
    echo "      Choose the same option to keep your config."
fi

echo ""
echo "============================================================"
echo " Starting fresh install..."
echo "============================================================"
echo ""

bash "$SCRIPT_DIR/install.sh"

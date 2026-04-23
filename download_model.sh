#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

export HF_HOME="$SCRIPT_DIR/models"
export HUGGINGFACE_HUB_CACHE="$SCRIPT_DIR/models"
export HF_HUB_ENABLE_HF_TRANSFER=1

if [ ! -f "venv/bin/python" ]; then
    echo "ERROR: Python venv not found! Run install.sh first."
    exit 1
fi

PYTHON="$SCRIPT_DIR/venv/bin/python"

echo "========================================"
echo "  ACE-Step Studio - Download Models"
echo "========================================"
echo ""
echo "Select model to download:"
echo ""
echo "  1. XL Turbo    - 18.8 GB, fast, 8 steps"
echo "  2. XL SFT      - 18.8 GB, best quality, 50 steps"
echo "  3. XL Turbo BF16 - 7.5 GB, compact, less VRAM"
echo "  4. Download all three"
echo ""
read -rp "Enter number 1-4: " MODEL_CHOICE

case "$MODEL_CHOICE" in
    1)
        echo ""
        echo "Downloading ACE-Step XL Turbo..."
        "$PYTHON" -m huggingface_hub.commands.huggingface_cli download \
            ACE-Step/acestep-v15-xl-turbo \
            --local-dir "ACE-Step-1.5/checkpoints/acestep-v15-xl-turbo"
        ;;
    2)
        echo ""
        echo "Downloading ACE-Step XL SFT..."
        "$PYTHON" -m huggingface_hub.commands.huggingface_cli download \
            ACE-Step/acestep-v15-xl-sft \
            --local-dir "ACE-Step-1.5/checkpoints/acestep-v15-xl-sft"
        ;;
    3)
        echo ""
        echo "Downloading ACE-Step XL Turbo BF16..."
        "$PYTHON" -m huggingface_hub.commands.huggingface_cli download \
            marcorez8/acestep-v15-xl-turbo-bf16 \
            --local-dir "ACE-Step-1.5/checkpoints/acestep-v15-xl-turbo-bf16"
        ;;
    4)
        echo ""
        echo "Downloading all three models..."
        echo ""
        echo "[1/3] XL Turbo..."
        "$PYTHON" -m huggingface_hub.commands.huggingface_cli download \
            ACE-Step/acestep-v15-xl-turbo \
            --local-dir "ACE-Step-1.5/checkpoints/acestep-v15-xl-turbo"
        echo ""
        echo "[2/3] XL SFT..."
        "$PYTHON" -m huggingface_hub.commands.huggingface_cli download \
            ACE-Step/acestep-v15-xl-sft \
            --local-dir "ACE-Step-1.5/checkpoints/acestep-v15-xl-sft"
        echo ""
        echo "[3/3] XL Turbo BF16..."
        "$PYTHON" -m huggingface_hub.commands.huggingface_cli download \
            marcorez8/acestep-v15-xl-turbo-bf16 \
            --local-dir "ACE-Step-1.5/checkpoints/acestep-v15-xl-turbo-bf16"
        ;;
    *)
        echo "Invalid choice!"
        exit 1
        ;;
esac

echo ""
echo "========================================"
echo "  Download complete!"
echo "========================================"

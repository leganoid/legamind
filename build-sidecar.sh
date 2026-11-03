#!/usr/bin/env bash
# Сборка автономного сайдкара legamind-engine (Linux/macOS).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYTHON="${LEGAMIND_PYTHON:-python3}"
DIST="$ROOT/sidecar-dist/legamind-engine"
WORK="$ROOT/.tools/pyinstaller-unix"

rm -rf "$DIST" "$WORK"
mkdir -p "$WORK"

cd "$ROOT"
"$PYTHON" -m PyInstaller \
  --noconfirm --clean --onedir \
  --name legamind-engine \
  --distpath "$ROOT/sidecar-dist" \
  --workpath "$WORK" \
  --specpath "$WORK" \
  --paths "$ROOT" \
  --collect-all openvino \
  --collect-all openvino_genai \
  --collect-all openvino_tokenizers \
  engine/sidecar_entry.py

chmod +x "$DIST/legamind-engine"
echo "Готово: $DIST"

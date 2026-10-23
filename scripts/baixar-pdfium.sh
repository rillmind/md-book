#!/usr/bin/env sh
# Baixa a biblioteca PDFium (motor de PDF do Chromium) para vendor/pdfium/.
# Binários de https://github.com/bblanchon/pdfium-binaries
set -eu

VERSAO="${PDFIUM_VERSAO:-7881}"
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) ALVO=linux-x64 ;;
  Linux-aarch64) ALVO=linux-arm64 ;;
  Darwin-arm64) ALVO=mac-arm64 ;;
  Darwin-x86_64) ALVO=mac-x64 ;;
  *) echo "plataforma não suportada: $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
DESTINO="$RAIZ/vendor/pdfium"
mkdir -p "$DESTINO"
curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$VERSAO/pdfium-$ALVO.tgz" \
  | tar -xz -C "$DESTINO"
echo "PDFium chromium/$VERSAO em $DESTINO/lib"

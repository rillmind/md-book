#!/usr/bin/env sh
# Atualiza o snapshot das configurações do Obsidian embutido no md-book a partir de um vault.
# Uso: scripts/atualizar-obsidian.sh [vault]   (padrão: ~/Documentos/Livros/md-books)
# Depois, recompile o md-book para embutir a versão nova.
set -eu

VAULT="${1:-$HOME/Documentos/Livros/md-books}"
RAIZ="$(cd "$(dirname "$0")/.." && pwd)"
DESTINO="$RAIZ/crates/core/obsidian"

if [ ! -d "$VAULT/.obsidian" ]; then
  echo "não há .obsidian em $VAULT" >&2
  exit 1
fi

rm -rf "$DESTINO"
mkdir -p "$DESTINO"
# Estado da janela (abas abertas) e conflitos de sincronização não são configuração.
(cd "$VAULT/.obsidian" && find . -type f \
  ! -name 'workspace*.json' \
  ! -name '*.sync-conflict-*' \
  -exec sh -c 'mkdir -p "$1/$(dirname "$2")" && cp "$2" "$1/$2"' _ "$DESTINO" {} \;)
echo "snapshot de $VAULT/.obsidian em $DESTINO:"
(cd "$DESTINO" && find . -type f | sort)

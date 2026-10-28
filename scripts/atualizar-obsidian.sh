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

# config.toml padrão: os plugins e temas da comunidade referenciados (id, repositório, versão),
# gerado pela mesma lógica do `md-book --save-config` (precisa de internet para o catálogo).
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
(cd "$RAIZ" && MD_BOOK_CONFIG="$TMP" cargo run -q --release -p mdbook-generator -- --save-config "$VAULT" >/dev/null)
{
  echo "# config.toml padrão embutido no md-book (gerado por scripts/atualizar-obsidian.sh)."
  grep -v '^#' "$TMP/config.toml" | sed '/./,$!d'
} > "$RAIZ/crates/core/obsidian-config.toml"
echo "config.toml padrão em $RAIZ/crates/core/obsidian-config.toml"

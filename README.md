# mdbook-generator

Gera um livro [mdBook](https://rust-lang.github.io/mdBook/) a partir de um PDF de texto:
`book.toml` + `src/SUMMARY.md` + um `.md` por capítulo.

```sh
cargo build --release                       # binário em target/release/md-book
./target/release/md-book livro.pdf -o meu-livro
mdbook serve meu-livro                      # ou use --build (abaixo)
```

| Opção | |
|---|---|
| `-o, --output <DIR>` | diretório do livro (padrão: `./<nome-do-pdf>`) |
| `--title`, `--author`, `--language` | campos do `book.toml` (título padrão: metadados do PDF) |
| `--split-level <N>` | títulos até o nível N viram arquivos separados (padrão 1) |
| `--no-outline` | ignora os bookmarks e detecta títulos pelo tamanho da fonte |
| `--force` | sobrescreve um diretório de saída não vazio (recria `src/`) |
| `--build` | compila o HTML; requer `cargo build --features build` |

## Como os capítulos são detectados

1. **Bookmarks do PDF**, se existirem e parecerem completos.
2. Senão, **tamanho da fonte**: o tamanho mais comum é o corpo; tamanhos maiores viram títulos.
3. Cabeçalhos/rodapés repetidos e números de página são removidos; linhas viram parágrafos
   (com hifenização desfeita) e marcadores (`•`, `-`) viram listas.

PDFs escaneados (só imagem) precisam de OCR antes.

## Estrutura

- `crates/core` — biblioteca (`analyze` → `Book` em memória, `write_book` → disco). TUI/GUI futuras usam só ela.
- `crates/cli` — binário `md-book`.
- `crates/core/tests/fixtures/gerar.py` — regenera os PDFs de teste (requer LibreOffice).

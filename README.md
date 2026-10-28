# md-book

Converte um PDF de texto em um livro [mdBook](https://rust-lang.github.io/mdBook/): gera o
`book.toml`, o `src/SUMMARY.md` e um arquivo Markdown por capítulo. Hoje é uma ferramenta de
linha de comando; TUI e GUI estão previstas.

## O que faz

Lê um PDF de texto, detecta os capítulos (pelos bookmarks do PDF, quando existem) e grava um
projeto mdBook pronto: `book.toml`, `src/SUMMARY.md` e um arquivo `NN-titulo.md` por capítulo.
Com `--build`, também gera o HTML do livro.

## Por que existe

Para ler livros e apostilas em PDF como Markdown: no navegador, pelo mdBook, ou no Obsidian,
que é o foco principal.

## Ferramentas

- [Rust](https://www.rust-lang.org/) (edition 2024) e Cargo
- [PDFium](https://pdfium.googlesource.com/pdfium/) (motor de PDF do Chromium) via [pdfium-render](https://crates.io/crates/pdfium-render): leitura do PDF
- [clap](https://crates.io/crates/clap) e [indicatif](https://crates.io/crates/indicatif): linha de comando e progresso
- [mdbook-driver](https://crates.io/crates/mdbook-driver): geração do HTML (opcional)

## Instalação

Requer o Rust instalado ([rustup](https://rustup.rs/)).

```sh
git clone https://github.com/rillmind/md-book
cd md-book
./scripts/baixar-pdfium.sh                         # baixa a libpdfium para vendor/pdfium
cargo install --path crates/cli                    # instala o binário md-book
cargo install --path crates/cli --features build   # idem, com suporte a --build (HTML)
```

O `md-book` procura a `libpdfium` na variável `MD_BOOK_PDFIUM`, ao lado do executável e em
`vendor/pdfium/lib` do repositório.

## Uso

```sh
md-book livro.pdf -o meu-livro           # gera o projeto mdBook em ./meu-livro
md-book livro.pdf -o meu-livro --build   # gera também o HTML em ./meu-livro/book
md-book livro.pdf -o ~/vault/Livro --flat  # só os capítulos .md e images/, numa pasta (Obsidian)
md-book livro.pdf -o ~/vault/Livro --full  # o livro inteiro num único .md, mais images/
md-book --help                           # todas as opções
```

Com `--flat`, a pasta de saída recebe apenas `NN-capitulo.md` e `images/`, sem `book.toml` nem
`SUMMARY.md`. Com `--full`, recebe um único `nome-do-livro.md` (capítulos como títulos `#`,
seções como `##`...) e `images/`. Nesses dois modos, mesmo com `--force`, nada que já esteja na
pasta é apagado: só os arquivos de mesmo nome são sobrescritos.

Sem instalar, rode direto do repositório: `cargo run --release -- livro.pdf -o meu-livro`.

## Como testar

```sh
./scripts/baixar-pdfium.sh   # uma vez, se ainda não baixou
cargo test
```

Roda os testes unitários do crate `core` e os testes de integração em
`crates/core/tests/fixtures.rs`, que convertem PDFs reais de `crates/core/tests/fixtures/`.

Para conferir à mão, converta uma das fixtures:

```sh
cargo run --release -- crates/core/tests/fixtures/outline-completo.pdf -o /tmp/livro
cat /tmp/livro/src/SUMMARY.md   # deve listar Início, 1 Introdução, 2 Instalação e Configuração, 3 Uso Avançado
```

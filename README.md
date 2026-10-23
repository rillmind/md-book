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
- [pdf-extract](https://crates.io/crates/pdf-extract) / [lopdf](https://crates.io/crates/lopdf): leitura do PDF
- [clap](https://crates.io/crates/clap) e [indicatif](https://crates.io/crates/indicatif): linha de comando e progresso
- [mdbook-driver](https://crates.io/crates/mdbook-driver): geração do HTML (opcional)

## Instalação

Requer o Rust instalado ([rustup](https://rustup.rs/)).

```sh
git clone <url-do-repositório>
cd mdbook-generator
cargo install --path crates/cli                    # instala o binário md-book
cargo install --path crates/cli --features build   # idem, com suporte a --build (HTML)
```

O clone cria a pasta `md-book` (o `cd mdbook-generator` acima é o nome da minha cópia local).
A partir de um clone limpo:

```sh
git clone https://github.com/rillmind/md-book.git
cd md-book
cargo install --path crates/cli
```

## Uso

```sh
md-book livro.pdf -o meu-livro           # gera o projeto mdBook em ./meu-livro
md-book livro.pdf -o meu-livro --build   # gera também o HTML em ./meu-livro/book
md-book --help                           # todas as opções
```

Sem instalar, rode direto do repositório: `cargo run --release -- livro.pdf -o meu-livro`.

## Como testar

```sh
cargo test
```

Roda os testes unitários do crate `core` e os testes de integração em
`crates/core/tests/fixtures.rs`, que convertem PDFs reais de `crates/core/tests/fixtures/`.

Para conferir à mão, converta uma das fixtures:

```sh
cargo run --release -- crates/core/tests/fixtures/outline-completo.pdf -o /tmp/livro
cat /tmp/livro/src/SUMMARY.md   # deve listar Início, 1 Introdução, 2 Instalação e Configuração, 3 Uso Avançado
```

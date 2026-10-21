# md-book

Converte um PDF de texto em um livro [mdBook](https://rust-lang.github.io/mdBook/): gera o
`book.toml`, o `src/SUMMARY.md` e um arquivo Markdown por capítulo. Hoje é uma ferramenta de
linha de comando; TUI e GUI estão previstas.

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

## Uso

```sh
md-book livro.pdf -o meu-livro           # gera o projeto mdBook em ./meu-livro
md-book livro.pdf -o meu-livro --build   # gera também o HTML em ./meu-livro/book
md-book --help                           # todas as opções
```

Sem instalar, rode direto do repositório: `cargo run --release -- livro.pdf -o meu-livro`.

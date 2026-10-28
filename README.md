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
md-book livro.pdf -o ~/vault/Livro --full  # o livro inteiro em book.md, mais images/
md-book --apply-config ~/Livros/A ~/Livros/B  # confere e aplica as configs do Obsidian em pastas existentes
md-book --help                           # todas as opções
```

Com `--flat`, a pasta de saída recebe apenas `NN-capitulo.md` e `images/`, sem `book.toml` nem
`SUMMARY.md`. Com `--full`, recebe um único `book.md` (capítulos como títulos `#`,
seções como `##`...) e `images/`. Nesses dois modos, mesmo com `--force`, nada que já esteja na
pasta é apagado: só os arquivos de mesmo nome são sobrescritos.

### Configurações do Obsidian

Toda pasta gerada já vem como vault do Obsidian: a pasta `.obsidian` recebe os temas, plugins,
atalhos, snippets e configurações padrão embutidos no `md-book` (lista e licenças em
[TERCEIROS.md](TERCEIROS.md)). Se a pasta já existe, o que falta é criado e o que difere do
padrão é restaurado; arquivos extras (outros plugins, abas abertas) nunca são apagados, e uma
pasta que só tem o `.obsidian` não exige `--force`.

- `--no-config`: não mexe no `.obsidian` (por exemplo, ao gerar dentro de um vault existente,
  para não criar um vault dentro de outro).
- `--apply-config <pastas>`: só confere e aplica as configurações, sem converter PDF; serve para
  livros gerados antes ou vaults que perderam alguma configuração. Não combina com as opções de
  conversão.
- `--save-config [vault]`: registra o `.obsidian` de um vault (padrão: a pasta atual) como o
  **seu** padrão, que passa a valer no lugar do embutido. Ele fica em `~/.config/md-book`
  (ou `$XDG_CONFIG_HOME/md-book`, ou `$MD_BOOK_CONFIG`), pronto para os dotfiles:

  ```
  ~/.config/md-book/
  ├── config.toml   # plugins e temas da comunidade: id, repositório e versão
  └── obsidian/     # o resto, como está: app.json, hotkeys.json, snippets, data.json dos
                    # plugins e plugins/temas que não estão na comunidade
  ```

  Plugins e temas da comunidade são baixados do GitHub na primeira vez que forem usados e
  guardados em `~/.cache/md-book`. Para trocar um plugin: troque no vault, rode
  `md-book --save-config` nele e, se quiser atualizar livros já gerados, `md-book --apply-config
  <pastas>`. Para voltar ao padrão embutido, apague `~/.config/md-book`.
- Padrão embutido: vem do vault de referência via `scripts/atualizar-obsidian.sh` (e recompilar).

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

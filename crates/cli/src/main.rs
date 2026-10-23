use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use mdbook_gen_core::{HeadingSource, Options, Progress};

/// Gera um livro mdBook (book.toml + src/SUMMARY.md + um .md por capítulo) a partir de um PDF de texto.
#[derive(Parser)]
#[command(name = "md-book", version)]
struct Cli {
  /// PDF de entrada.
  input: PathBuf,

  /// Diretório do livro [padrão: ./<nome-do-pdf>]
  #[arg(short, long)]
  output: Option<PathBuf>,

  /// Título do livro [padrão: metadados do PDF ou nome do arquivo]
  #[arg(long)]
  title: Option<String>,

  /// Autor (pode repetir).
  #[arg(long = "author")]
  authors: Vec<String>,

  /// Idioma do livro no book.toml (ex.: pt-BR).
  #[arg(long)]
  language: Option<String>,

  /// Títulos até este nível viram arquivos separados (1 = só capítulos) [padrão: automático]
  #[arg(long, value_parser = clap::value_parser!(u8).range(1..=6))]
  split_level: Option<u8>,

  /// Ignora os bookmarks do PDF e detecta títulos só pelo tamanho da fonte.
  #[arg(long)]
  no_outline: bool,

  /// Sobrescreve o diretório de saída se ele não estiver vazio.
  #[arg(long)]
  force: bool,

  /// Também compila o livro em HTML (requer a feature `build`).
  #[arg(long)]
  build: bool,
}

fn main() -> Result<()> {
  let cli = Cli::parse();

  if cli.build && !cfg!(feature = "build") {
    anyhow::bail!(
      "--build não está disponível neste binário; recompile com `--features build` \
             ou rode `mdbook build` no diretório gerado"
    );
  }

  let options = Options {
    use_outline: !cli.no_outline,
    split_level: cli.split_level,
    title: cli.title,
    authors: cli.authors,
    language: cli.language,
  };
  let output = cli.output.unwrap_or_else(|| default_output(&cli.input));

  let bar = ProgressBar::new(0).with_style(
    ProgressStyle::with_template("{spinner} {msg} [{bar:30}] {pos}/{len}")
      .unwrap()
      .progress_chars("=> "),
  );
  bar.set_message("Lendo páginas");
  let book = mdbook_gen_core::analyze(&cli.input, &options, |p| match p {
    Progress::Page { done, total } => {
      bar.set_length(total.into());
      bar.set_position(done.into());
    }
    Progress::Structuring => bar.set_message("Montando capítulos"),
  })
  .with_context(|| format!("falha ao converter {}", cli.input.display()));
  bar.finish_and_clear();
  let book = book?;

  let written = mdbook_gen_core::write_book(&book, &output, cli.force)?;

  let source = match book.heading_source {
    HeadingSource::Outline => "bookmarks do PDF",
    HeadingSource::FontSize => "tamanho da fonte",
    HeadingSource::None => "nenhum título detectado",
  };
  println!(
    "{} capítulo(s) em {} (títulos: {source})",
    written.chapter_files.len(),
    written.root.display()
  );
  if book.outline_ignored {
    eprintln!("aviso: os bookmarks do PDF estavam incompletos e foram ignorados");
  }
  if !book.failed_pages.is_empty() {
    eprintln!(
      "aviso: páginas ignoradas por erro de leitura: {:?}",
      book.failed_pages
    );
  }

  if cli.build {
    build_html(&written.root)?;
  }
  Ok(())
}

fn default_output(input: &Path) -> PathBuf {
  PathBuf::from(input.file_stem().unwrap_or(input.as_os_str()))
}

#[cfg(feature = "build")]
fn build_html(root: &Path) -> Result<()> {
  let book = mdbook_driver::MDBook::load(root).context("falha ao carregar o livro gerado")?;
  book.build().context("falha ao compilar o livro")?;
  println!(
    "HTML em {}",
    book.build_dir_for("html").join("index.html").display()
  );
  Ok(())
}

#[cfg(not(feature = "build"))]
fn build_html(_root: &Path) -> Result<()> {
  unreachable!("--build é rejeitado antes sem a feature `build`")
}

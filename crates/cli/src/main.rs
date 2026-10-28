use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use mdbook_gen_core::{ConfigReport, HeadingSource, Options, OutputLayout, Progress, WriteOptions};

/// Gera um livro mdBook (book.toml + src/SUMMARY.md + um .md por capítulo) a partir de um PDF de texto.
///
/// A pasta gerada já vem como vault do Obsidian, com temas, plugins, atalhos e snippets padrão
/// em .obsidian (use --no-config para não criar).
#[derive(Parser)]
#[command(name = "md-book", version)]
struct Cli {
  /// PDF de entrada.
  #[arg(required_unless_present = "apply_config")]
  input: Option<PathBuf>,

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

  /// Só os capítulos .md e a pasta images/, direto no diretório de saída, sem book.toml nem
  /// SUMMARY.md (ex.: para uma pasta do Obsidian). Com --force, nada é apagado: só os arquivos
  /// de mesmo nome são sobrescritos.
  #[arg(long, conflicts_with_all = ["build", "full"])]
  flat: bool,

  /// O livro inteiro num único arquivo book.md, mais a pasta images/, no diretório de saída.
  /// Com --force, só sobrescreve esse arquivo e as imagens.
  #[arg(long, conflicts_with = "build")]
  full: bool,

  /// Não cria nem atualiza as configurações do Obsidian (.obsidian) na pasta gerada; útil ao
  /// gerar dentro de um vault que já existe, para não criar um vault dentro de outro.
  #[arg(long)]
  no_config: bool,

  /// Confere e aplica as configurações padrão do Obsidian em pastas já existentes, sem converter
  /// nenhum PDF: cria o que falta e restaura o que difere, sem apagar arquivos extras.
  #[arg(
    long,
    value_name = "DIR",
    num_args = 1..,
    conflicts_with_all = [
      "input", "output", "title", "authors", "language", "split_level", "no_outline",
      "force", "build", "flat", "full", "no_config",
    ]
  )]
  apply_config: Vec<PathBuf>,
}

fn main() -> Result<()> {
  let cli = Cli::parse();
  if !cli.apply_config.is_empty() {
    return apply_config(&cli.apply_config);
  }
  let input = cli
    .input
    .expect("clap exige o PDF quando não há --apply-config");

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
  let output = cli.output.unwrap_or_else(|| default_output(&input));

  let bar = ProgressBar::new(0).with_style(
    ProgressStyle::with_template("{spinner} {msg} [{bar:30}] {pos}/{len}")
      .unwrap()
      .progress_chars("=> "),
  );
  bar.set_message("Lendo páginas");
  let book = mdbook_gen_core::analyze(&input, &options, |p| match p {
    Progress::Page { done, total } => {
      bar.set_length(total.into());
      bar.set_position(done.into());
    }
    Progress::Structuring => bar.set_message("Montando capítulos"),
  })
  .with_context(|| format!("falha ao converter {}", input.display()));
  bar.finish_and_clear();
  let book = book?;

  let layout = match (cli.flat, cli.full) {
    (true, _) => OutputLayout::Flat,
    (_, true) => OutputLayout::Full,
    _ => OutputLayout::MdBook,
  };
  let write = WriteOptions {
    force: cli.force,
    layout,
    obsidian: !cli.no_config,
  };
  let written = mdbook_gen_core::write_book(&book, &output, &write)?;

  let source = match book.heading_source {
    HeadingSource::Outline => "bookmarks do PDF",
    HeadingSource::FontSize => "tamanho da fonte",
    HeadingSource::None => "nenhum título detectado",
  };
  // No modo --full o destino é o arquivo único; nos outros, o diretório.
  let destination = match layout {
    OutputLayout::Full => written.chapter_files[0].display(),
    _ => written.root.display(),
  };
  println!(
    "{} capítulo(s) e {} imagem(ns) em {destination} (títulos: {source})",
    book.chapters.len(),
    written.images,
  );
  if let Some(report) = &written.obsidian {
    println!("configurações do Obsidian: {}", describe(report));
  }
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

/// Confere e aplica as configurações do Obsidian em cada pasta; falha se alguma não existir.
fn apply_config(dirs: &[PathBuf]) -> Result<()> {
  let mut failed = false;
  for dir in dirs {
    if !dir.is_dir() {
      eprintln!("{}: não é uma pasta existente", dir.display());
      failed = true;
      continue;
    }
    let report = mdbook_gen_core::apply_obsidian(dir)
      .with_context(|| format!("falha ao aplicar as configurações em {}", dir.display()))?;
    println!("{}: {}", dir.display(), describe(&report));
    for path in report.missing.iter().chain(&report.outdated) {
      println!("    {}", path.display());
    }
  }
  if failed {
    anyhow::bail!("algumas pastas não existem");
  }
  Ok(())
}

fn describe(report: &ConfigReport) -> String {
  if report.is_up_to_date() {
    "já estavam no padrão".to_string()
  } else {
    format!(
      "{} arquivo(s) criado(s), {} restaurado(s) ao padrão",
      report.missing.len(),
      report.outdated.len()
    )
  }
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

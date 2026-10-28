use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use mdbook_gen_core::{
  ConfigReport, HeadingSource, ObsidianOutcome, Options, OutputLayout, Progress, Template,
  WriteOptions,
};

/// Gera um livro mdBook (book.toml + src/SUMMARY.md + um .md por capítulo) a partir de um PDF de texto.
///
/// A pasta gerada já vem como vault do Obsidian, com temas, plugins, atalhos e snippets padrão
/// em .obsidian (use --no-config para não criar). O padrão é o embutido no md-book, ou o seu,
/// se você registrou um com --save-config (em ~/.config/md-book).
#[derive(Parser)]
#[command(name = "md-book", version)]
struct Cli {
  /// PDF de entrada.
  #[arg(required_unless_present_any = ["apply_config", "save_config"])]
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
      "force", "build", "flat", "full", "no_config", "save_config",
    ]
  )]
  apply_config: Vec<PathBuf>,

  /// Registra as configurações do Obsidian deste vault [padrão: pasta atual] como o seu padrão,
  /// em ~/.config/md-book (config.toml + obsidian/), para guardar nos dotfiles. Plugins e temas
  /// da comunidade ficam só referenciados (id, repositório, versão) e são baixados quando usados;
  /// os demais arquivos são guardados como estão. Substitui o padrão registrado antes.
  #[arg(
    long,
    value_name = "VAULT",
    num_args = 0..=1,
    default_missing_value = ".",
    conflicts_with_all = [
      "input", "output", "title", "authors", "language", "split_level", "no_outline",
      "force", "build", "flat", "full", "no_config",
    ]
  )]
  save_config: Option<PathBuf>,
}

fn main() -> Result<()> {
  let cli = Cli::parse();
  if let Some(vault) = &cli.save_config {
    return save_config(vault);
  }
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
    obsidian: (!cli.no_config).then(Template::detect),
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
  match (&written.obsidian, &write.obsidian) {
    (Some(ObsidianOutcome::Applied(report)), Some(template)) => println!(
      "configurações do Obsidian ({}): {}",
      template_name(template),
      describe(report)
    ),
    (Some(ObsidianOutcome::InsideVault(vault)), _) => println!(
      "configurações do Obsidian: não criadas, a pasta já está dentro do vault {}",
      vault.display()
    ),
    _ => {}
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

/// Registra o `.obsidian` do vault como padrão do usuário.
fn save_config(vault: &Path) -> Result<()> {
  let dir = mdbook_gen_core::config_dir().context(
    "não foi possível descobrir o diretório de configuração (defina HOME ou MD_BOOK_CONFIG)",
  )?;
  let report = mdbook_gen_core::save_config(vault, &dir)
    .with_context(|| format!("falha ao registrar as configurações de {}", vault.display()))?;
  println!("padrão do Obsidian registrado em {}", dir.display());
  let list = |items: &[String]| {
    if items.is_empty() {
      "nenhum".to_string()
    } else {
      items.join(", ")
    }
  };
  println!(
    "  plugins da comunidade (baixados quando usados): {}",
    list(&report.plugins)
  );
  println!(
    "  plugins locais (guardados inteiros): {}",
    list(&report.local_plugins)
  );
  println!("  temas da comunidade: {}", list(&report.themes));
  if !report.local_themes.is_empty() {
    println!("  temas locais: {}", list(&report.local_themes));
  }
  println!(
    "  {} arquivo(s) de configuração guardados em obsidian/",
    report.files
  );
  Ok(())
}

fn template_name(template: &Template) -> String {
  match template {
    Template::Embedded => "padrão embutido".to_string(),
    Template::User(dir) => format!("padrão de {}", dir.display()),
  }
}

/// Confere e aplica as configurações do Obsidian em cada pasta; falha se alguma não existir.
fn apply_config(dirs: &[PathBuf]) -> Result<()> {
  let template = Template::detect();
  println!("usando o {}", template_name(&template));
  let mut failed = false;
  for dir in dirs {
    if !dir.is_dir() {
      eprintln!("{}: não é uma pasta existente", dir.display());
      failed = true;
      continue;
    }
    let report = match mdbook_gen_core::ensure_obsidian(dir, &template)
      .with_context(|| format!("falha ao aplicar as configurações em {}", dir.display()))?
    {
      ObsidianOutcome::Applied(report) => report,
      ObsidianOutcome::InsideVault(vault) => {
        println!(
          "{}: nada feito, a pasta fica dentro do vault {} (para conferir o vault: md-book --apply-config {})",
          dir.display(),
          vault.display(),
          vault.display()
        );
        continue;
      }
    };
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

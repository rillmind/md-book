//! Núcleo do mdbook-generator: converte um PDF de texto num projeto mdBook.
//!
//! O fluxo é dividido em duas etapas para que CLI, TUI e GUI possam mostrar uma prévia:
//! [`analyze`] lê o PDF e monta o [`Book`] em memória; [`write_book`] grava em disco.

mod book;
mod config;
mod extract;
mod figures;
mod furniture;
mod layout;
mod markdown;
mod model;
mod obsidian;
mod outline;
mod pdfium;
mod structure;
mod tables;
mod text;

use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use book::{FULL_FILE, OutputLayout, WriteOptions, WrittenBook, write_book};
pub use config::{
  CACHE_ENV, CONFIG_ENV, SaveReport, config_dir, ensure_default as ensure_default_config,
  save as save_config,
};
pub use markdown::render_chapter;
pub use model::Style;
pub use obsidian::{
  CONFIG_DIR as OBSIDIAN_DIR, ConfigReport, Outcome as ObsidianOutcome, Template,
  apply as apply_obsidian, check as check_obsidian, enclosing_vault, ensure as ensure_obsidian,
};
pub use pdfium::PDFIUM_ENV;
pub use structure::{Block, Chapter, FRONT_MATTER_TITLE, HeadingSource, Inline};
pub use text::ListKind;

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("não foi possível abrir o PDF: {0}")]
  Pdf(String),
  #[error("o PDF está protegido por senha")]
  Encrypted,
  #[error(
    "biblioteca PDFium não encontrada ({0}); rode scripts/baixar-pdfium.sh ou defina MD_BOOK_PDFIUM"
  )]
  PdfiumMissing(String),
  #[error("nenhum texto encontrado no PDF; se ele for escaneado (imagens), passe por um OCR antes")]
  NoText,
  #[error("o diretório de saída {0} não está vazio (use --force para sobrescrever)")]
  OutputNotEmpty(PathBuf),
  #[error("configuração inválida: {0}")]
  Config(String),
  #[error("falha ao baixar da comunidade do Obsidian: {0}")]
  Network(String),
  #[error(transparent)]
  Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct Options {
  /// Usa os bookmarks do PDF para achar capítulos, quando existirem.
  pub use_outline: bool,
  /// Títulos de nível `<= split_level` começam um novo arquivo; `None` escolhe sozinho
  /// (capítulos, ou capítulos dentro de partes).
  pub split_level: Option<u8>,
  /// Sobrescreve o título vindo dos metadados do PDF.
  pub title: Option<String>,
  pub authors: Vec<String>,
  pub language: Option<String>,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      use_outline: true,
      split_level: None,
      title: None,
      authors: Vec::new(),
      language: None,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
  /// Página `done` de `total` extraída.
  Page {
    done: u32,
    total: u32,
  },
  Structuring,
}

/// Livro em memória, pronto para ser revisado ou gravado.
#[derive(Debug, Clone)]
pub struct Book {
  pub title: String,
  pub authors: Vec<String>,
  pub language: Option<String>,
  pub chapters: Vec<Chapter>,
  pub heading_source: HeadingSource,
  /// O PDF tinha bookmarks, mas incompletos; os títulos vieram do tamanho da fonte.
  pub outline_ignored: bool,
  /// Páginas que o parser não conseguiu ler (ficaram de fora).
  pub failed_pages: Vec<u32>,
  /// Diretório temporário com as imagens extraídas; apagado quando o livro é descartado.
  pub assets: Option<Arc<tempfile::TempDir>>,
}

/// Lê o PDF e monta o livro.
pub fn analyze(
  input: &Path,
  options: &Options,
  mut progress: impl FnMut(Progress),
) -> Result<Book, Error> {
  let document = pdfium::pdfium()?
    .load_pdf_from_file(input, None)
    .map_err(|e| match e {
      pdfium_render::prelude::PdfiumError::PdfiumLibraryInternalError(
        pdfium_render::prelude::PdfiumInternalError::PasswordError,
      ) => Error::Encrypted,
      e => Error::Pdf(e.to_string()),
    })?;
  let assets = tempfile::Builder::new().prefix("md-book-").tempdir()?;

  let extracted = extract::extract(&document, assets.path(), |done, total| {
    progress(Progress::Page { done, total })
  });
  let failed_pages = extracted.failed;
  let mut pages = extracted.pages;
  if pages.iter().all(|p| p.lines.is_empty()) {
    return Err(Error::NoText);
  }

  progress(Progress::Structuring);
  furniture::remove_headers_and_footers(&mut pages);
  furniture::remove_reference_pages(&mut pages);
  let body = body_size(&pages);
  let tables: Vec<Vec<tables::Table>> = pages
    .iter_mut()
    .map(|page| tables::extract_tables(page, body))
    .collect();
  let ignore = figures::Ignore {
    images: figures::decorative_assets(&extracted.assets),
    paths: figures::repeated_paths(&pages),
  };
  let mut candidates: Vec<_> = pages
    .iter()
    .zip(&tables)
    .map(|(page, tables)| {
      let taken: Vec<model::Rect> = tables.iter().map(|t| t.bbox).collect();
      figures::detect(page, &ignore, &taken, body)
    })
    .collect();
  figures::drop_repeated(&mut candidates);
  let contents: Vec<layout::PageContent> = pages
    .into_iter()
    .zip(candidates)
    .zip(tables)
    .map(|((mut page, candidates), tables)| {
      let found = figures::extract_figures(
        &document,
        &mut page,
        candidates,
        &extracted.assets,
        body,
        assets.path(),
      );
      layout::arrange(page, found, tables)
    })
    .collect();
  let outline = if options.use_outline {
    outline::read_outline(&document)
  } else {
    Vec::new()
  };
  let structured = structure::build_blocks(&contents, &outline);
  drop(contents);

  let title = options
    .title
    .clone()
    .or_else(|| outline::read_title(&document))
    .unwrap_or_else(|| file_stem(input));

  let split_level = options
    .split_level
    .unwrap_or_else(|| structure::auto_split_level(&structured.blocks))
    .max(1);
  let mut chapters = structure::split_chapters(structured.blocks, split_level);
  // Sem nenhum capítulo detectado, o livro inteiro leva o nome do livro.
  if let [only] = chapters.as_mut_slice()
    && only.title == FRONT_MATTER_TITLE
  {
    only.title = title.clone();
  }

  Ok(Book {
    title,
    authors: options.authors.clone(),
    language: options.language.clone(),
    chapters,
    heading_source: structured.source,
    outline_ignored: structured.outline_ignored,
    failed_pages,
    assets: Some(Arc::new(assets)),
  })
}

/// Tamanho de fonte do corpo do texto: o mais frequente, por quantidade de caracteres.
fn body_size(pages: &[model::Page]) -> f32 {
  let mut chars: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
  for line in pages.iter().flat_map(|p| &p.lines) {
    *chars.entry((line.size * 2.0).round() as u32).or_default() += line.char_count();
  }
  chars
    .into_iter()
    .max_by_key(|&(size, n)| (n, size))
    .map_or(0.0, |(size, _)| size as f32 / 2.0)
}

fn file_stem(path: &Path) -> String {
  path
    .file_stem()
    .map(|s| s.to_string_lossy().into_owned())
    .unwrap_or_else(|| "Livro".into())
}

/// Diagnóstico: linhas extraídas de uma página, com posição, tamanho e estilo de cada trecho.
#[doc(hidden)]
pub fn debug_page(input: &Path, page: u32) -> Result<String, Error> {
  use std::fmt::Write as _;
  let document = pdfium::pdfium()?
    .load_pdf_from_file(input, None)
    .map_err(|e| Error::Pdf(e.to_string()))?;
  let assets = tempfile::tempdir()?;
  let extracted = extract::extract(&document, assets.path(), |_, _| {});
  let mut out = String::new();
  let Some(p) = extracted.pages.get(page.saturating_sub(1) as usize) else {
    return Ok(out);
  };
  for line in &p.lines {
    writeln!(
      out,
      "[{:6.1} {:6.1} {:6.1} base={:6.1} size={:4.1}] {}",
      line.bbox.x0,
      line.bbox.x1,
      line.bbox.y0,
      line.baseline,
      line.size,
      line.text()
    )
    .unwrap();
    for span in &line.spans {
      let s = span.style;
      writeln!(
        out,
        "    {:6.1}-{:6.1} {:4.1} {}{}{} {:?}",
        span.bbox.x0,
        span.bbox.x1,
        span.size,
        if s.bold { "B" } else { "-" },
        if s.italic { "I" } else { "-" },
        if s.mono { "M" } else { "-" },
        span.text
      )
      .unwrap();
    }
  }
  for g in &p.graphics {
    writeln!(out, "graphic {:?} {:?}", g.kind, g.bbox).unwrap();
  }
  Ok(out)
}

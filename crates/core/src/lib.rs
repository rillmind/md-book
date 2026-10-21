//! Núcleo do mdbook-generator: converte um PDF de texto num projeto mdBook.
//!
//! O fluxo é dividido em duas etapas para que CLI, TUI e GUI possam mostrar uma prévia:
//! [`analyze`] lê o PDF e monta o [`Book`] em memória; [`write_book`] grava em disco.

mod book;
mod extract;
mod markdown;
mod outline;
mod structure;

use std::path::{Path, PathBuf};

pub use book::{WrittenBook, write_book};
pub use markdown::render_chapter;
pub use structure::{Block, Chapter, FRONT_MATTER_TITLE, HeadingSource};

#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("não foi possível abrir o PDF: {0}")]
  Pdf(#[from] pdf_extract::Error),
  #[error("o PDF está criptografado; descriptografe-o antes de converter")]
  Encrypted,
  #[error("nenhum texto encontrado no PDF; se ele for escaneado (imagens), passe por um OCR antes")]
  NoText,
  #[error("o diretório de saída {0} não está vazio (use --force para sobrescrever)")]
  OutputNotEmpty(PathBuf),
  #[error(transparent)]
  Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct Options {
  /// Usa os bookmarks do PDF para achar capítulos, quando existirem.
  pub use_outline: bool,
  /// Títulos de nível `<= split_level` começam um novo arquivo.
  pub split_level: u8,
  /// Sobrescreve o título vindo dos metadados do PDF.
  pub title: Option<String>,
  pub authors: Vec<String>,
  pub language: Option<String>,
}

impl Default for Options {
  fn default() -> Self {
    Self {
      use_outline: true,
      split_level: 1,
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
}

/// Lê o PDF e monta o livro.
pub fn analyze(
  input: &Path,
  options: &Options,
  mut progress: impl FnMut(Progress),
) -> Result<Book, Error> {
  let doc = pdf_extract::Document::load(input)?;
  if doc.is_encrypted() {
    return Err(Error::Encrypted);
  }

  let (mut pages, failed_pages) =
    extract::extract_pages(&doc, |done, total| progress(Progress::Page { done, total }));
  if pages.iter().all(|p| p.lines.is_empty()) {
    return Err(Error::NoText);
  }

  progress(Progress::Structuring);
  structure::remove_page_furniture(&mut pages);
  let outline = if options.use_outline {
    outline::read_outline(&doc)
  } else {
    Vec::new()
  };
  let structured = structure::build_blocks(&pages, &outline);
  drop(pages);

  let title = options
    .title
    .clone()
    .or_else(|| outline::read_title(&doc))
    .unwrap_or_else(|| file_stem(input));

  let mut chapters = structure::split_chapters(structured.blocks, options.split_level.max(1));
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
  })
}

fn file_stem(path: &Path) -> String {
  path
    .file_stem()
    .map(|s| s.to_string_lossy().into_owned())
    .unwrap_or_else(|| "Livro".into())
}

//! Metadados do PDF: outline (bookmarks) e título.

use pdfium_render::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
  /// Nível na árvore de bookmarks, começando em 1.
  pub level: u8,
  pub title: String,
  /// Página de destino, começando em 1.
  pub page: u32,
}

/// Bookmarks do PDF em ordem de leitura. Vazio se o PDF não tiver outline.
pub fn read_outline(document: &PdfDocument) -> Vec<OutlineEntry> {
  let mut entries = Vec::new();
  let mut node = document.bookmarks().root();
  while let Some(bookmark) = node {
    node = bookmark.next_sibling();
    walk(&bookmark, 1, &mut entries);
  }
  // A árvore já está em ordem de leitura; garantimos ordem por página sem quebrar empates.
  entries.sort_by_key(|e| e.page);
  entries
}

fn walk(bookmark: &PdfBookmark, level: u8, out: &mut Vec<OutlineEntry>) {
  let title = bookmark
    .title()
    .unwrap_or_default()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
  if let Some(page) = destination_page(bookmark)
    && !title.is_empty()
  {
    out.push(OutlineEntry {
      level: level.min(6),
      title,
      page,
    });
  }
  let mut child = bookmark.first_child();
  while let Some(c) = child {
    child = c.next_sibling();
    walk(&c, level.saturating_add(1), out);
  }
}

fn destination_page(bookmark: &PdfBookmark) -> Option<u32> {
  let index = match bookmark.destination() {
    Some(dest) => dest.page_index().ok()?,
    None => match bookmark.action()? {
      PdfAction::LocalDestination(action) => action.destination().ok()?.page_index().ok()?,
      _ => return None,
    },
  };
  u32::try_from(index).ok().map(|i| i + 1)
}

/// Título dos metadados do PDF, se existir e não for vazio.
pub fn read_title(document: &PdfDocument) -> Option<String> {
  let tag = document
    .metadata()
    .get(PdfDocumentMetadataTagType::Title)?;
  let title = tag.value().trim();
  (!title.is_empty()).then(|| title.to_string())
}

//! Metadados do PDF: outline (bookmarks) e título.

use pdf_extract::{Document, Object};

#[derive(Debug, Clone, PartialEq)]
pub struct OutlineEntry {
  /// Nível na árvore de bookmarks, começando em 1.
  pub level: u8,
  pub title: String,
  /// Página de destino, começando em 1.
  pub page: u32,
}

/// Bookmarks do PDF em ordem de leitura. Vazio se o PDF não tiver outline.
pub fn read_outline(doc: &Document) -> Vec<OutlineEntry> {
  let Ok(toc) = doc.get_toc() else {
    return Vec::new();
  };
  let mut entries: Vec<OutlineEntry> = toc
    .toc
    .into_iter()
    .filter(|e| !e.title.trim().is_empty())
    .map(|e| OutlineEntry {
      level: e.level.clamp(1, 6) as u8,
      title: e.title.split_whitespace().collect::<Vec<_>>().join(" "),
      page: e.page as u32,
    })
    .collect();
  // O get_toc preserva a ordem da árvore; garantimos ordem por página sem quebrar empates.
  entries.sort_by_key(|e| e.page);
  entries
}

/// Valor de `/Title` no dicionário `/Info`, se existir e não for vazio.
pub fn read_title(doc: &Document) -> Option<String> {
  let info = doc.trailer.get(b"Info").ok()?;
  let info = match info {
    Object::Reference(id) => doc.get_object(*id).ok()?,
    other => other,
  };
  let title = info.as_dict().ok()?.get(b"Title").ok()?;
  let bytes = match title {
    Object::String(bytes, _) => bytes,
    _ => return None,
  };
  let title = decode_pdf_string(bytes);
  let title = title.trim();
  (!title.is_empty()).then(|| title.to_string())
}

fn decode_pdf_string(bytes: &[u8]) -> String {
  if let [0xfe, 0xff, rest @ ..] = bytes {
    let units: Vec<u16> = rest
      .chunks_exact(2)
      .map(|c| u16::from_be_bytes([c[0], c[1]]))
      .collect();
    String::from_utf16_lossy(&units)
  } else if let Ok(s) = std::str::from_utf8(bytes) {
    s.to_string()
  } else {
    // PDFDocEncoding coincide com Latin-1 nos caracteres que importam aqui.
    bytes.iter().map(|&b| b as char).collect()
  }
}

//! PDFs reais gerados pelo LibreOffice (ver `fixtures/gerar.py`).

use std::path::PathBuf;

use mdbook_gen_core::{Block, Book, HeadingSource, Options, analyze, write_book};

const CHAPTERS: [&str; 3] = [
  "1 Introdução",
  "2 Instalação e Configuração",
  "3 Uso Avançado",
];

fn fixture(name: &str) -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("tests/fixtures")
    .join(name)
}

fn convert(name: &str) -> Book {
  analyze(&fixture(name), &Options::default(), |_| {}).unwrap()
}

/// Cada capítulo do fixture tem 2 parágrafos, uma seção, uma lista de 2 itens e outra seção.
fn assert_chapters(book: &Book) {
  let titles: Vec<&str> = book.chapters.iter().map(|c| c.title.as_str()).collect();
  assert_eq!(&titles[1..], CHAPTERS);

  for (chapter, n) in book.chapters[1..].iter().zip(1..) {
    let headings: Vec<&str> = chapter
      .blocks
      .iter()
      .filter_map(|b| match b {
        Block::Heading { text, .. } => Some(text.as_str()),
        _ => None,
      })
      .collect();
    assert_eq!(
      headings,
      [format!("{n}.1 Primeiros passos"), format!("{n}.2 Detalhes")]
    );

    let items = chapter
      .blocks
      .iter()
      .filter(|b| matches!(b, Block::ListItem { .. }))
      .count();
    let paragraphs = chapter
      .blocks
      .iter()
      .filter(|b| matches!(b, Block::Paragraph(_)))
      .count();
    assert_eq!(items, 2, "{}", chapter.title);
    assert_eq!(paragraphs, 5, "{}", chapter.title);
  }
}

#[test]
fn uses_complete_outline() {
  let book = convert("outline-completo.pdf");
  assert_eq!(book.heading_source, HeadingSource::Outline);
  assert!(!book.outline_ignored);
  assert_eq!(book.title, "Guia de Teste");
  assert_chapters(&book);
}

#[test]
fn falls_back_to_font_size_when_outline_is_incomplete() {
  let book = convert("outline-incompleto.pdf");
  assert_eq!(book.heading_source, HeadingSource::FontSize);
  assert!(book.outline_ignored);
  assert_chapters(&book);
}

#[test]
fn detects_chapters_by_font_size_without_outline() {
  let book = convert("sem-outline.pdf");
  assert_eq!(book.heading_source, HeadingSource::FontSize);
  assert_chapters(&book);
}

#[test]
fn writes_mdbook_project() {
  let book = convert("outline-completo.pdf");
  let out = std::env::temp_dir().join(format!("mdbook-gen-test-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&out);

  let written = write_book(&book, &out, false).unwrap();
  assert_eq!(written.chapter_files.len(), 4);
  let summary = std::fs::read_to_string(out.join("src/SUMMARY.md")).unwrap();
  assert!(summary.contains("- [1 Introdução](./02-1-introducao.md)"));
  let toml = std::fs::read_to_string(out.join("book.toml")).unwrap();
  assert!(toml.contains("title = \"Guia de Teste\""));

  // Diretório não vazio só é sobrescrito com force.
  assert!(write_book(&book, &out, false).is_err());
  write_book(&book, &out, true).unwrap();

  std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn extracts_photo_and_vector_figure_with_captions() {
  let book = convert("figuras.pdf");
  let blocks: Vec<&Block> = book.chapters.iter().flat_map(|c| &c.blocks).collect();
  let images: Vec<(&String, &String)> = blocks
    .iter()
    .filter_map(|b| match b {
      Block::Image { file, alt } => Some((file, alt)),
      _ => None,
    })
    .collect();
  assert_eq!(images.len(), 2, "{blocks:?}");
  // A foto é copiada como veio no PDF; o diagrama é renderizado.
  assert!(images[0].0.starts_with("img-"));
  assert!(images[1].0.starts_with("fig-") && images[1].0.ends_with(".png"));
  assert!(images[0].1.starts_with("Figura 1.1"));
  assert!(images[1].1.starts_with("Figura 1.2"));

  // Os rótulos do diagrama ficam só na imagem (a legenda continua no texto).
  let paragraphs: Vec<String> = blocks
    .iter()
    .filter_map(|b| match b {
      Block::Paragraph(content) => Some(format!("{content:?}")),
      _ => None,
    })
    .filter(|p| !p.contains("Figura"))
    .collect();
  assert!(
    paragraphs
      .iter()
      .all(|p| !p.contains("Cliente") && !p.contains("Servidor"))
  );

  let out = std::env::temp_dir().join(format!("mdbook-gen-fig-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&out);
  let written = write_book(&book, &out, false).unwrap();
  assert_eq!(written.images, 2);
  assert_eq!(
    std::fs::read_dir(out.join("src/images")).unwrap().count(),
    2
  );
  std::fs::remove_dir_all(&out).unwrap();
}

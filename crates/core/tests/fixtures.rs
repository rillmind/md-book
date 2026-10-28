//! PDFs reais gerados pelo LibreOffice (ver `fixtures/gerar.py`).

use std::path::PathBuf;

use mdbook_gen_core::{
  Block, Book, HeadingSource, Options, OutputLayout, Template, WriteOptions, analyze, write_book,
};

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

fn opts(force: bool, layout: OutputLayout) -> WriteOptions {
  WriteOptions {
    force,
    layout,
    obsidian: Some(Template::Embedded),
  }
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

  let written = write_book(&book, &out, &opts(false, OutputLayout::MdBook)).unwrap();
  assert_eq!(written.chapter_files.len(), 4);
  let summary = std::fs::read_to_string(out.join("src/SUMMARY.md")).unwrap();
  assert!(summary.contains("- [1 Introdução](./02-1-introducao.md)"));
  let toml = std::fs::read_to_string(out.join("book.toml")).unwrap();
  assert!(toml.contains("title = \"Guia de Teste\""));

  // Diretório não vazio só é sobrescrito com force.
  assert!(write_book(&book, &out, &opts(false, OutputLayout::MdBook)).is_err());
  write_book(&book, &out, &opts(true, OutputLayout::MdBook)).unwrap();

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
  let written = write_book(&book, &out, &opts(false, OutputLayout::MdBook)).unwrap();
  assert_eq!(written.images, 2);
  assert_eq!(
    std::fs::read_dir(out.join("src/images")).unwrap().count(),
    2
  );
  std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn converts_bordered_table() {
  let book = convert("tabela.pdf");
  let rows = book
    .chapters
    .iter()
    .flat_map(|c| &c.blocks)
    .find_map(|b| match b {
      Block::Table(rows) => Some(rows),
      _ => None,
    })
    .expect("tabela");
  let text = |cell: &Vec<mdbook_gen_core::Inline>| -> String {
    cell
      .iter()
      .map(|i| match i {
        mdbook_gen_core::Inline::Text { text, .. } => text.clone(),
        _ => String::new(),
      })
      .collect()
  };
  let cells: Vec<Vec<String>> = rows.iter().map(|r| r.iter().map(text).collect()).collect();
  assert_eq!(
    cells,
    [
      ["Comando", "Função"],
      ["ls", "Lista arquivos"],
      [
        "cp",
        "Copia arquivos e diretórios entre dois lugares quaisquer do sistema"
      ],
      ["rm", "Remove arquivos"],
    ]
  );
}

#[test]
fn flat_layout_writes_only_chapters_and_images() {
  let book = convert("figuras.pdf");
  let out = std::env::temp_dir().join(format!("mdbook-gen-flat-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&out);
  std::fs::create_dir_all(&out).unwrap();
  // Uma nota que já existia na pasta (ex.: vault do Obsidian) não pode ser apagada.
  std::fs::write(out.join("minha-nota.md"), "não apagar").unwrap();

  assert!(write_book(&book, &out, &opts(false, OutputLayout::Flat)).is_err());
  let written = write_book(&book, &out, &opts(true, OutputLayout::Flat)).unwrap();
  assert!(!out.join("book.toml").exists() && !out.join("src").exists());
  assert!(!out.join("SUMMARY.md").exists());
  assert_eq!(written.images, 2);
  assert_eq!(std::fs::read_dir(out.join("images")).unwrap().count(), 2);
  let chapter = std::fs::read_to_string(&written.chapter_files[0]).unwrap();
  assert!(chapter.contains("](images/"));
  assert_eq!(
    std::fs::read_to_string(out.join("minha-nota.md")).unwrap(),
    "não apagar"
  );

  std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn full_layout_writes_the_whole_book_in_one_file() {
  let book = convert("outline-completo.pdf");
  let out = std::env::temp_dir().join(format!("mdbook-gen-full-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&out);

  let written = write_book(&book, &out, &opts(false, OutputLayout::Full)).unwrap();
  assert_eq!(written.chapter_files, [out.join("book.md")]);
  let mut entries: Vec<String> = std::fs::read_dir(&out)
    .unwrap()
    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
    .collect();
  entries.sort();
  assert_eq!(
    entries,
    [".obsidian", "book.md"],
    "o fixture não tem imagens"
  );

  let text = std::fs::read_to_string(&written.chapter_files[0]).unwrap();
  let headings: Vec<&str> = text.lines().filter(|l| l.starts_with('#')).collect();
  assert_eq!(
    &headings[..4],
    [
      "# Início",
      "# 1 Introdução",
      "## 1.1 Primeiros passos",
      "## 1.2 Detalhes"
    ]
  );
  assert!(headings.contains(&"# 3 Uso Avançado"));
  std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn every_layout_gets_the_obsidian_settings_unless_disabled() {
  let book = convert("outline-completo.pdf");
  for layout in [OutputLayout::MdBook, OutputLayout::Flat, OutputLayout::Full] {
    let out =
      std::env::temp_dir().join(format!("mdbook-gen-obs-{layout:?}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let written = write_book(&book, &out, &opts(false, layout)).unwrap();
    assert!(written.obsidian.is_some_and(|r| !r.missing.is_empty()));
    assert!(
      mdbook_gen_core::check_obsidian(&out, &Template::Embedded)
        .unwrap()
        .is_up_to_date(),
      "{layout:?}"
    );
    assert!(out.join(".obsidian/hotkeys.json").exists());

    // Rodar de novo na mesma pasta: as configurações já estão lá.
    let again = write_book(&book, &out, &opts(true, layout)).unwrap();
    assert!(again.obsidian.unwrap().is_up_to_date());
    std::fs::remove_dir_all(&out).unwrap();
  }

  let out = std::env::temp_dir().join(format!("mdbook-gen-noobs-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&out);
  let options = WriteOptions {
    obsidian: None,
    ..opts(false, OutputLayout::Flat)
  };
  let written = write_book(&book, &out, &options).unwrap();
  assert!(written.obsidian.is_none() && !out.join(".obsidian").exists());
  std::fs::remove_dir_all(&out).unwrap();
}

#[test]
fn folder_with_only_obsidian_settings_does_not_need_force() {
  let book = convert("outline-completo.pdf");
  let out = std::env::temp_dir().join(format!("mdbook-gen-prep-{}", std::process::id()));
  let _ = std::fs::remove_dir_all(&out);
  std::fs::create_dir_all(&out).unwrap();
  mdbook_gen_core::apply_obsidian(&out, &Template::Embedded).unwrap();
  write_book(&book, &out, &opts(false, OutputLayout::MdBook)).unwrap();
  std::fs::remove_dir_all(&out).unwrap();
}

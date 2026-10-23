//! Escrita do projeto mdBook em disco.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::markdown::{IMAGES_DIR, escape_link_text, render_chapter};
use crate::structure::Block;
use crate::{Book, Error};

/// Arquivos gerados por [`write_book`].
#[derive(Debug, Clone)]
pub struct WrittenBook {
  pub root: PathBuf,
  pub chapter_files: Vec<PathBuf>,
  /// Imagens copiadas para `src/images`.
  pub images: usize,
}

/// Escreve `book.toml`, `src/SUMMARY.md`, um arquivo por capítulo e as imagens em `out_dir`.
///
/// Recusa um diretório não vazio, a menos que `force` seja verdadeiro; nesse caso
/// `src/` é recriado do zero para não sobrarem capítulos antigos.
pub fn write_book(book: &Book, out_dir: &Path, force: bool) -> Result<WrittenBook, Error> {
  let not_empty = out_dir.exists() && fs::read_dir(out_dir)?.next().is_some();
  if not_empty && !force {
    return Err(Error::OutputNotEmpty(out_dir.to_path_buf()));
  }
  let src = out_dir.join("src");
  if src.exists() {
    fs::remove_dir_all(&src)?;
  }
  fs::create_dir_all(&src)?;

  fs::write(out_dir.join("book.toml"), book_toml(book))?;

  let names = chapter_file_names(book);
  let mut summary = String::from("# Summary\n\n");
  let mut chapter_files = Vec::with_capacity(names.len());
  for (chapter, name) in book.chapters.iter().zip(&names) {
    let indent = "    ".repeat(chapter.depth.saturating_sub(1) as usize);
    let title = escape_link_text(&chapter.title);
    writeln!(summary, "{indent}- [{title}](./{name})").unwrap();

    let path = src.join(name);
    fs::write(&path, render_chapter(chapter))?;
    chapter_files.push(path);
  }
  fs::write(src.join("SUMMARY.md"), summary)?;

  let images = copy_images(book, &src.join(IMAGES_DIR))?;
  Ok(WrittenBook {
    root: out_dir.to_path_buf(),
    chapter_files,
    images,
  })
}

/// Copia do diretório de recursos só as imagens que algum capítulo usa.
fn copy_images(book: &Book, dest: &Path) -> Result<usize, Error> {
  let Some(assets) = &book.assets else {
    return Ok(0);
  };
  let mut copied = std::collections::HashSet::new();
  for block in book.chapters.iter().flat_map(|c| &c.blocks) {
    if let Block::Image { file, .. } = block
      && copied.insert(file.clone())
    {
      fs::create_dir_all(dest)?;
      fs::copy(assets.path().join(file), dest.join(file))?;
    }
  }
  Ok(copied.len())
}

fn book_toml(book: &Book) -> String {
  let mut toml = String::from("[book]\n");
  writeln!(toml, "title = {}", toml_string(&book.title)).unwrap();
  let authors: Vec<String> = book.authors.iter().map(|a| toml_string(a)).collect();
  writeln!(toml, "authors = [{}]", authors.join(", ")).unwrap();
  if let Some(language) = &book.language {
    writeln!(toml, "language = {}", toml_string(language)).unwrap();
  }
  toml.push_str("src = \"src\"\n");
  toml
}

fn toml_string(s: &str) -> String {
  let mut out = String::from("\"");
  for c in s.chars() {
    match c {
      '"' => out.push_str("\\\""),
      '\\' => out.push_str("\\\\"),
      c if c.is_control() => write!(out, "\\u{:04X}", c as u32).unwrap(),
      c => out.push(c),
    }
  }
  out.push('"');
  out
}

/// `01-introducao.md`, `02-outro-capitulo.md`, ...
fn chapter_file_names(book: &Book) -> Vec<String> {
  let width = book.chapters.len().to_string().len().max(2);
  book
    .chapters
    .iter()
    .enumerate()
    .map(|(i, c)| format!("{:0width$}-{}.md", i + 1, slug(&c.title)))
    .collect()
}

fn slug(title: &str) -> String {
  let ascii = deunicode::deunicode(title).to_lowercase();
  let mut slug = String::new();
  for c in ascii.chars() {
    if c.is_ascii_alphanumeric() {
      slug.push(c);
    } else if !slug.is_empty() && !slug.ends_with('-') {
      slug.push('-');
    }
    if slug.len() >= 50 {
      break;
    }
  }
  let slug = slug.trim_end_matches('-');
  if slug.is_empty() {
    "capitulo".to_string()
  } else {
    slug.to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn slugs_are_ascii_and_bounded() {
    assert_eq!(slug("Capítulo 1: Introdução!"), "capitulo-1-introducao");
    assert_eq!(slug("???"), "capitulo");
    assert!(slug(&"a b ".repeat(100)).len() <= 50);
  }

  #[test]
  fn toml_strings_are_escaped() {
    assert_eq!(
      toml_string(r#"Diz "oi" \ tchau"#),
      r#""Diz \"oi\" \\ tchau""#
    );
  }
}

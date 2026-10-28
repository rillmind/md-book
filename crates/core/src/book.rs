//! Escrita do livro em disco: projeto mdBook (padrão) ou só os capítulos numa pasta.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use crate::markdown::{IMAGES_DIR, escape_link_text, render_chapter, render_chapter_at};
use crate::obsidian::{self, Outcome, Template};
use crate::structure::Block;
use crate::{Book, Error};

/// Formato da saída.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputLayout {
  /// Projeto mdBook: `book.toml`, `src/SUMMARY.md`, capítulos e imagens em `src/`.
  #[default]
  MdBook,
  /// Só os capítulos `.md` e a pasta `images/`, direto no diretório de saída (ex.: dentro de um
  /// vault do Obsidian).
  Flat,
  /// O livro inteiro num único `book.md` e a pasta `images/`.
  Full,
}

/// Nome do arquivo único gerado com [`OutputLayout::Full`].
pub const FULL_FILE: &str = "book.md";

/// Como gravar o livro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOptions {
  /// Aceita um diretório de saída que já tem conteúdo.
  pub force: bool,
  pub layout: OutputLayout,
  /// Configurações do Obsidian a garantir em `.obsidian` (temas, plugins, atalhos); `None` não
  /// mexe nelas.
  pub obsidian: Option<Template>,
}

impl Default for WriteOptions {
  fn default() -> Self {
    Self {
      force: false,
      layout: OutputLayout::default(),
      obsidian: Some(Template::Embedded),
    }
  }
}

/// Arquivos gerados por [`write_book`].
#[derive(Debug, Clone)]
pub struct WrittenBook {
  pub root: PathBuf,
  pub chapter_files: Vec<PathBuf>,
  /// Imagens copiadas para a pasta `images`.
  pub images: usize,
  /// O que foi feito em `.obsidian`, quando as configurações estão ligadas.
  pub obsidian: Option<Outcome>,
}

/// Escreve o livro em `out_dir`.
///
/// Recusa um diretório com conteúdo (fora a pasta `.obsidian`), a menos que `force` seja
/// verdadeiro. No formato mdBook, `src/` é recriado do zero para não sobrarem capítulos antigos;
/// nos formatos plano e completo nada é apagado (a pasta pode ter outras notas), só os arquivos
/// de mesmo nome são sobrescritos. A pasta `.obsidian` nunca é apagada.
pub fn write_book(
  book: &Book,
  out_dir: &Path,
  options: &WriteOptions,
) -> Result<WrittenBook, Error> {
  let not_empty = out_dir.exists()
    && fs::read_dir(out_dir)?
      .filter_map(Result::ok)
      .any(|e| e.file_name() != obsidian::CONFIG_DIR);
  if not_empty && !options.force {
    return Err(Error::OutputNotEmpty(out_dir.to_path_buf()));
  }
  fs::create_dir_all(out_dir)?;

  let (chapter_files, images) = match options.layout {
    OutputLayout::MdBook => write_mdbook(book, out_dir)?,
    OutputLayout::Flat => write_chapters(book, out_dir)?,
    OutputLayout::Full => write_full(book, out_dir)?,
  };
  let obsidian = options
    .obsidian
    .as_ref()
    .map(|template| obsidian::ensure(out_dir, template))
    .transpose()?;
  Ok(WrittenBook {
    root: out_dir.to_path_buf(),
    chapter_files,
    images,
    obsidian,
  })
}

fn write_mdbook(book: &Book, out_dir: &Path) -> Result<(Vec<PathBuf>, usize), Error> {
  let src = out_dir.join("src");
  if src.exists() {
    fs::remove_dir_all(&src)?;
  }
  fs::create_dir_all(&src)?;
  fs::write(out_dir.join("book.toml"), book_toml(book))?;
  let (files, images) = write_chapters(book, &src)?;

  let mut summary = String::from("# Summary\n\n");
  for (chapter, path) in book.chapters.iter().zip(&files) {
    let indent = "    ".repeat(chapter.depth.saturating_sub(1) as usize);
    let title = escape_link_text(&chapter.title);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    writeln!(summary, "{indent}- [{title}](./{name})").unwrap();
  }
  fs::write(src.join("SUMMARY.md"), summary)?;
  Ok((files, images))
}

/// Um `NN-titulo.md` por capítulo e a pasta `images/` em `dir`.
fn write_chapters(book: &Book, dir: &Path) -> Result<(Vec<PathBuf>, usize), Error> {
  let mut files = Vec::with_capacity(book.chapters.len());
  for (chapter, name) in book.chapters.iter().zip(chapter_file_names(book)) {
    let path = dir.join(name);
    fs::write(&path, render_chapter(chapter))?;
    files.push(path);
  }
  let images = copy_images(book, &dir.join(IMAGES_DIR))?;
  Ok((files, images))
}

/// O livro inteiro em `book.md`; capítulos como títulos no nível da sua profundidade.
fn write_full(book: &Book, dir: &Path) -> Result<(Vec<PathBuf>, usize), Error> {
  let path = dir.join(FULL_FILE);
  let text: Vec<String> = book
    .chapters
    .iter()
    .map(|c| render_chapter_at(c, c.depth))
    .collect();
  fs::write(&path, text.join("\n"))?;
  let images = copy_images(book, &dir.join(IMAGES_DIR))?;
  Ok((vec![path], images))
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

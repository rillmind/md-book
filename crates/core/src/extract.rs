//! Extração de texto do PDF, linha a linha, preservando tamanho de fonte e posição vertical.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use pdf_extract::{Document, MediaBox, OutputDev, OutputError, Transform};

/// Uma linha visual de texto.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
  pub text: String,
  /// Tamanho de fonte dominante da linha (em pontos, arredondado a 0.5).
  pub font_size: f32,
  /// Posição vertical a partir do topo da página.
  pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
  /// Número da página, começando em 1.
  pub number: u32,
  pub lines: Vec<Line>,
}

/// Extrai todas as páginas. Páginas que o parser não consegue ler viram páginas vazias
/// e são reportadas em `failed`.
pub fn extract_pages(doc: &Document, mut on_page: impl FnMut(u32, u32)) -> (Vec<Page>, Vec<u32>) {
  let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
  let total = numbers.len() as u32;
  let mut pages = Vec::with_capacity(numbers.len());
  let mut failed = Vec::new();

  for (i, &number) in numbers.iter().enumerate() {
    let mut collector = LineCollector::default();
    // pdf-extract ainda tem `unwrap`s internos; uma página malformada não deve derrubar o livro.
    let result = catch_unwind(AssertUnwindSafe(|| {
      pdf_extract::output_doc_page(doc, &mut collector, number)
    }));
    match result {
      Ok(Ok(())) => {}
      _ => failed.push(number),
    }
    collector.flush();
    pages.push(Page {
      number,
      lines: collector.lines,
    });
    on_page(i as u32 + 1, total);
  }
  (pages, failed)
}

#[derive(Default)]
struct LineCollector {
  page_height: f64,
  lines: Vec<Line>,
  text: String,
  /// Contagem de caracteres por tamanho de fonte (chave = tamanho * 2).
  sizes: HashMap<u32, usize>,
  y: f64,
  last_end: f64,
  has_line: bool,
}

impl LineCollector {
  fn flush(&mut self) {
    let text = self.text.trim();
    if !text.is_empty() {
      let size = self
        .sizes
        .iter()
        .max_by_key(|&(size, count)| (*count, *size))
        .map(|(size, _)| *size as f32 / 2.0)
        .unwrap_or(0.0);
      self.lines.push(Line {
        text: collapse_spaces(text),
        font_size: size,
        y: self.y as f32,
      });
    }
    self.text.clear();
    self.sizes.clear();
    self.has_line = false;
  }
}

impl OutputDev for LineCollector {
  fn begin_page(
    &mut self,
    _page_num: u32,
    media_box: &MediaBox,
    _art_box: Option<(f64, f64, f64, f64)>,
  ) -> Result<(), OutputError> {
    self.page_height = media_box.ury - media_box.lly;
    Ok(())
  }

  fn end_page(&mut self) -> Result<(), OutputError> {
    self.flush();
    Ok(())
  }

  fn output_character(
    &mut self,
    trm: &Transform,
    width: f64,
    _spacing: f64,
    font_size: f64,
    char: &str,
  ) -> Result<(), OutputError> {
    // Mesmo cálculo do PlainTextOutput do pdf-extract: tamanho efetivo após a matriz de texto.
    let sx = font_size * trm.m11 + font_size * trm.m21;
    let sy = font_size * trm.m12 + font_size * trm.m22;
    let size = (sx * sy).abs().sqrt();
    let x = trm.m31;
    let y = self.page_height - trm.m32;

    if self.has_line {
      let new_line = (y - self.y).abs() > size * 0.5;
      if new_line {
        self.flush();
      } else if x > self.last_end + size * 0.1 && !self.text.ends_with(' ') {
        self.text.push(' ');
      }
    }
    if !self.has_line {
      self.y = y;
      self.has_line = true;
    }

    self.text.push_str(char);
    if !char.trim().is_empty() {
      *self.sizes.entry((size * 2.0).round() as u32).or_default() += char.chars().count();
    }
    self.last_end = x + width * size;
    Ok(())
  }

  fn begin_word(&mut self) -> Result<(), OutputError> {
    Ok(())
  }

  fn end_word(&mut self) -> Result<(), OutputError> {
    Ok(())
  }

  fn end_line(&mut self) -> Result<(), OutputError> {
    Ok(())
  }
}

fn collapse_spaces(s: &str) -> String {
  s.split_whitespace().collect::<Vec<_>>().join(" ")
}

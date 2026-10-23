//! Ordem de leitura de cada página: quais elementos (linhas, figuras, tabelas) vêm antes.
//!
//! Páginas de uma coluna seguem a ordem do fluxo de conteúdo do PDF. Em páginas com colunas, a
//! calha (faixa vertical que nenhuma linha atravessa) divide a página: linhas que cruzam a calha
//! (títulos, figuras largas) separam faixas, e cada faixa é lida coluna por coluna.

use crate::figures::Figure;
use crate::model::{Line, Page};
use crate::tables::Table;

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
  Line(Line),
  Figure(Figure),
  Table(Table),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PageContent {
  pub number: u32,
  pub width: f32,
  pub height: f32,
  pub elements: Vec<Element>,
}

impl PageContent {
  pub fn lines(&self) -> impl Iterator<Item = &Line> {
    self.elements.iter().filter_map(|e| match e {
      Element::Line(line) => Some(line),
      _ => None,
    })
  }
}

/// Largura mínima da calha entre colunas.
const MIN_GUTTER: f32 = 6.0;
/// Colunas com menos linhas que isso não são colunas (rótulos, notas de margem).
const MIN_COLUMN_LINES: usize = 4;
/// Profundidade máxima de colunas dentro de colunas.
const MAX_COLUMN_DEPTH: usize = 3;

impl Element {
  fn bbox(&self) -> crate::model::Rect {
    match self {
      Element::Line(line) => line.bbox,
      Element::Figure(figure) => figure.bbox,
      Element::Table(table) => table.bbox,
    }
  }
}

/// Põe linhas, figuras e tabelas em ordem de leitura.
pub fn arrange(page: Page, figures: Vec<Figure>, tables: Vec<Table>) -> PageContent {
  let mut elements: Vec<Element> = page.lines.into_iter().map(Element::Line).collect();
  elements.extend(figures.into_iter().map(Element::Figure));
  elements.extend(tables.into_iter().map(Element::Table));
  PageContent {
    number: page.number,
    width: page.width,
    height: page.height,
    elements: reading_order(elements, MAX_COLUMN_DEPTH),
  }
}

fn reading_order(elements: Vec<Element>, depth: usize) -> Vec<Element> {
  let gutter = if depth == 0 {
    None
  } else {
    let lines: Vec<&Line> = elements
      .iter()
      .filter_map(|e| match e {
        Element::Line(l) => Some(l),
        _ => None,
      })
      .collect();
    find_gutter(&lines)
  };
  let Some(gutter) = gutter else {
    return stream_order(elements);
  };

  let mut sorted = elements;
  sorted.sort_by(|a, b| a.bbox().y0.total_cmp(&b.bbox().y0));
  let mut out = Vec::with_capacity(sorted.len());
  let (mut left, mut right): (Vec<Element>, Vec<Element>) = (Vec::new(), Vec::new());
  let flush = |out: &mut Vec<Element>,
               left: &mut Vec<Element>,
               right: &mut Vec<Element>,
               next_top: Option<f32>| {
    // Elementos colados logo acima do que fecha a faixa e abaixo do fim da outra coluna (ex.:
    // legenda acima de uma figura larga) vão junto com ele, depois das duas colunas.
    let mut tail = Vec::new();
    if let Some(top) = next_top
      && !left.is_empty()
      && !right.is_empty()
    {
      let bottom = |side: &[Element]| side.iter().map(|e| e.bbox().y1).fold(f32::MIN, f32::max);
      let (left_bottom, right_bottom) = (bottom(left), bottom(right));
      let attached = |e: &Element, other_bottom: f32| {
        let b = e.bbox();
        b.y0 > other_bottom + 2.0 && top - b.y1 < 30.0
      };
      let (moved, kept): (Vec<Element>, Vec<Element>) = std::mem::take(left)
        .into_iter()
        .partition(|e| attached(e, right_bottom));
      *left = kept;
      tail.extend(moved);
      let (moved, kept): (Vec<Element>, Vec<Element>) = std::mem::take(right)
        .into_iter()
        .partition(|e| attached(e, left_bottom));
      *right = kept;
      tail.extend(moved);
      tail.sort_by(|a, b| a.bbox().y0.total_cmp(&b.bbox().y0));
    }
    out.extend(reading_order(std::mem::take(left), depth - 1));
    out.extend(reading_order(std::mem::take(right), depth - 1));
    out.extend(tail);
  };
  for element in sorted {
    let bbox = element.bbox();
    if bbox.x1 <= gutter {
      left.push(element);
    } else if bbox.x0 >= gutter {
      right.push(element);
    } else {
      // O que cruza a calha (título, figura larga) fecha a faixa de colunas acima dele.
      flush(&mut out, &mut left, &mut right, Some(bbox.y0));
      out.push(element);
    }
  }
  flush(&mut out, &mut left, &mut right, None);
  out
}

/// Sem colunas: linhas na ordem do fluxo do PDF, cada figura antes da primeira linha abaixo do
/// topo dela (na mesma faixa horizontal).
fn stream_order(elements: Vec<Element>) -> Vec<Element> {
  let (mut out, figures): (Vec<Element>, Vec<Element>) = elements
    .into_iter()
    .partition(|e| matches!(e, Element::Line(_)));
  for figure in figures {
    let f = figure.bbox();
    let at = out
      .iter()
      .position(|e| {
        let l = e.bbox();
        matches!(e, Element::Line(_)) && l.y0 >= f.y0 && l.x0 < f.x1 && f.x0 < l.x1
      })
      .unwrap_or(out.len());
    out.insert(at, figure);
  }
  out
}

/// Posição x da calha entre duas colunas, se a página tiver colunas.
fn find_gutter(lines: &[&Line]) -> Option<f32> {
  if lines.len() < MIN_COLUMN_LINES * 2 {
    return None;
  }
  let x_min = lines.iter().map(|l| l.bbox.x0).fold(f32::MAX, f32::min);
  let x_max = lines.iter().map(|l| l.bbox.x1).fold(f32::MIN, f32::max);
  let width = x_max - x_min;
  if width <= MIN_GUTTER * 4.0 {
    return None;
  }

  // Quantas linhas cobrem cada faixa de 2 pt. Linhas largas (títulos, rodapés, legendas) cruzam
  // a calha de propósito e não contam.
  let bins = (width / 2.0).ceil() as usize + 1;
  let mut coverage = vec![0usize; bins];
  for line in lines.iter().filter(|l| l.bbox.width() < width * 0.6) {
    let a = ((line.bbox.x0 - x_min) / 2.0).floor() as usize;
    let b = (((line.bbox.x1 - x_min) / 2.0).ceil() as usize).min(bins - 1);
    for c in &mut coverage[a..=b] {
      *c += 1;
    }
  }
  // Tolera algumas linhas atravessando (palavras longas, rótulos).
  let tolerance = (lines.len() / 25).max(1);
  let (from, to) = ((bins as f32 * 0.2) as usize, (bins as f32 * 0.8) as usize);
  let mut best: Option<(usize, usize)> = None; // (início, comprimento)
  let mut run_start = None;
  for (i, &count) in coverage.iter().enumerate().take(to + 1).skip(from) {
    let low = count <= tolerance;
    match (low, run_start) {
      (true, None) => run_start = Some(i),
      (false, Some(start)) => {
        if best.is_none_or(|(_, len)| i - start > len) {
          best = Some((start, i - start));
        }
        run_start = None;
      }
      _ => {}
    }
  }
  if let Some(start) = run_start
    && best.is_none_or(|(_, len)| to + 1 - start > len)
  {
    best = Some((start, to + 1 - start));
  }
  let (start, len) = best?;
  if len as f32 * 2.0 < MIN_GUTTER {
    return None;
  }
  let gutter = x_min + (start as f32 + len as f32 / 2.0) * 2.0;

  // As duas colunas precisam ter texto e correr lado a lado.
  let left: Vec<&Line> = lines
    .iter()
    .copied()
    .filter(|l| l.bbox.x1 <= gutter)
    .collect();
  let right: Vec<&Line> = lines
    .iter()
    .copied()
    .filter(|l| l.bbox.x0 >= gutter)
    .collect();
  if left.len() < MIN_COLUMN_LINES || right.len() < MIN_COLUMN_LINES {
    return None;
  }
  let span = |ls: &[&Line]| {
    let top = ls.iter().map(|l| l.bbox.y0).fold(f32::MAX, f32::min);
    let bottom = ls.iter().map(|l| l.bbox.y1).fold(f32::MIN, f32::max);
    (top, bottom)
  };
  let ((lt, lb), (rt, rb)) = (span(&left), span(&right));
  let overlap = lb.min(rb) - lt.max(rt);
  let shorter = (lb - lt).min(rb - rt);
  (overlap >= shorter * 0.3).then_some(gutter)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{Rect, Span, Style};

  fn line(text: &str, x0: f32, x1: f32, y0: f32) -> Line {
    let bbox = Rect::new(x0, y0, x1, y0 + 10.0);
    Line {
      spans: vec![Span {
        text: text.into(),
        bbox,
        size: 10.0,
        baseline: y0 + 8.0,
        style: Style::default(),
        space_before: false,
      }],
      bbox,
      size: 10.0,
      baseline: y0 + 8.0,
    }
  }

  #[test]
  fn reads_two_columns_after_a_spanning_title() {
    // Fluxo do PDF intercalado e título no fim, como em aberturas de capítulo.
    let mut lines = Vec::new();
    for i in 0..5 {
      lines.push(line(&format!("E{i}"), 50.0, 280.0, 200.0 + i as f32 * 14.0));
      lines.push(line(
        &format!("D{i}"),
        300.0,
        540.0,
        200.0 + i as f32 * 14.0,
      ));
    }
    lines.push(line("Título largo", 150.0, 450.0, 100.0));
    lines.push(line("Rodapé largo", 50.0, 540.0, 400.0));
    let elements = lines.into_iter().map(Element::Line).collect();
    let ordered: Vec<String> = reading_order(elements, MAX_COLUMN_DEPTH)
      .iter()
      .map(|e| match e {
        Element::Line(l) => l.text(),
        Element::Figure(f) => f.file.clone(),
        Element::Table(_) => "tabela".into(),
      })
      .collect();
    assert_eq!(
      ordered,
      [
        "Título largo",
        "E0",
        "E1",
        "E2",
        "E3",
        "E4",
        "D0",
        "D1",
        "D2",
        "D3",
        "D4",
        "Rodapé largo"
      ]
    );
  }

  #[test]
  fn single_column_keeps_stream_order() {
    let lines: Vec<Line> = (0..10)
      .map(|i| {
        line(
          &format!("L{i}"),
          50.0,
          if i % 3 == 2 { 200.0 } else { 540.0 },
          100.0 + i as f32 * 14.0,
        )
      })
      .collect();
    assert!(find_gutter(&lines.iter().collect::<Vec<_>>()).is_none());
  }

  #[test]
  fn wide_figure_separates_column_bands() {
    let mut elements: Vec<Element> = Vec::new();
    for i in 0..5 {
      elements.push(Element::Line(line(
        &format!("E{i}"),
        50.0,
        280.0,
        100.0 + i as f32 * 14.0,
      )));
      elements.push(Element::Line(line(
        &format!("D{i}"),
        300.0,
        540.0,
        100.0 + i as f32 * 14.0,
      )));
    }
    elements.push(Element::Figure(Figure {
      bbox: Rect::new(50.0, 200.0, 450.0, 300.0),
      file: "fig".into(),
      alt: String::new(),
    }));
    elements.push(Element::Line(line("Legenda acima", 50.0, 200.0, 185.0)));
    let ordered: Vec<String> = reading_order(elements, MAX_COLUMN_DEPTH)
      .iter()
      .map(|e| match e {
        Element::Line(l) => l.text(),
        Element::Figure(f) => f.file.clone(),
        Element::Table(_) => "tabela".into(),
      })
      .collect();
    assert_eq!(&ordered[4..7], ["E4", "D0", "D1"]);
    assert_eq!(&ordered[10..], ["Legenda acima", "fig"]);
  }

  #[test]
  fn figure_goes_before_the_line_below_it_in_its_column() {
    let page = Page {
      number: 1,
      width: 600.0,
      height: 800.0,
      lines: vec![
        line("acima", 50.0, 540.0, 100.0),
        line("abaixo", 50.0, 540.0, 300.0),
      ],
      graphics: Vec::new(),
    };
    let figure = Figure {
      bbox: Rect::new(100.0, 150.0, 400.0, 280.0),
      file: "f.png".into(),
      alt: String::new(),
    };
    let content = arrange(page, vec![figure], Vec::new());
    assert!(matches!(content.elements[1], Element::Figure(_)));
  }
}

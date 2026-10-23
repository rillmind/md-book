//! Ordem de leitura de cada página: quais elementos (linhas, figuras, tabelas) vêm antes.

use crate::figures::Figure;
use crate::model::{Line, Page};

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
  Line(Line),
  Figure(Figure),
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

/// Usa a ordem do fluxo de conteúdo do PDF, que nos documentos de uma coluna já é a de leitura,
/// e encaixa cada figura antes da primeira linha que fica abaixo do topo dela.
pub fn arrange(page: Page, figures: Vec<Figure>) -> PageContent {
  let mut elements: Vec<Element> = page.lines.into_iter().map(Element::Line).collect();
  for figure in figures {
    let at = elements
      .iter()
      .position(|e| matches!(e, Element::Line(l) if l.bbox.y0 >= figure.bbox.y0))
      .unwrap_or(elements.len());
    elements.insert(at, Element::Figure(figure));
  }
  PageContent {
    number: page.number,
    width: page.width,
    height: page.height,
    elements,
  }
}

//! Ordem de leitura de cada página: quais elementos (linhas, figuras, tabelas) vêm antes.

use crate::model::{Line, Page};

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
  Line(Line),
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
    self.elements.iter().map(|e| match e {
      Element::Line(line) => line,
    })
  }
}

/// Usa a ordem do fluxo de conteúdo do PDF, que nos documentos de uma coluna já é a de leitura.
pub fn arrange(page: Page) -> PageContent {
  PageContent {
    number: page.number,
    width: page.width,
    height: page.height,
    elements: page.lines.into_iter().map(Element::Line).collect(),
  }
}

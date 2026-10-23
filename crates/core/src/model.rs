//! Modelo intermediário: o que foi extraído de cada página, em coordenadas com origem no
//! canto superior esquerdo (y cresce para baixo), em pontos.

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
  pub x0: f32,
  pub y0: f32,
  pub x1: f32,
  pub y1: f32,
}

impl Rect {
  pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
    Self {
      x0: x0.min(x1),
      y0: y0.min(y1),
      x1: x0.max(x1),
      y1: y0.max(y1),
    }
  }

  pub fn width(&self) -> f32 {
    self.x1 - self.x0
  }

  pub fn height(&self) -> f32 {
    self.y1 - self.y0
  }

  pub fn area(&self) -> f32 {
    self.width() * self.height()
  }

  pub fn center(&self) -> (f32, f32) {
    ((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
  }

  pub fn union(&self, other: &Rect) -> Rect {
    Rect {
      x0: self.x0.min(other.x0),
      y0: self.y0.min(other.y0),
      x1: self.x1.max(other.x1),
      y1: self.y1.max(other.y1),
    }
  }

  pub fn expand(&self, pad: f32) -> Rect {
    Rect {
      x0: self.x0 - pad,
      y0: self.y0 - pad,
      x1: self.x1 + pad,
      y1: self.y1 + pad,
    }
  }

  pub fn intersects(&self, other: &Rect) -> bool {
    self.x0 <= other.x1 && other.x0 <= self.x1 && self.y0 <= other.y1 && other.y0 <= self.y1
  }

  pub fn contains_point(&self, (x, y): (f32, f32)) -> bool {
    x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Style {
  pub bold: bool,
  pub italic: bool,
  pub mono: bool,
}

/// Trecho contínuo de uma linha com o mesmo estilo e tamanho.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
  pub text: String,
  pub bbox: Rect,
  pub size: f32,
  pub baseline: f32,
  pub style: Style,
  /// Havia espaço (real ou inferido pelo PDFium) antes deste trecho.
  pub space_before: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
  pub spans: Vec<Span>,
  pub bbox: Rect,
  /// Tamanho de fonte dominante (por quantidade de caracteres).
  pub size: f32,
  pub baseline: f32,
}

impl Line {
  pub fn text(&self) -> String {
    let mut text = String::new();
    for span in &self.spans {
      if span.space_before && !text.is_empty() && !text.ends_with(' ') {
        text.push(' ');
      }
      text.push_str(&span.text);
    }
    text.trim().to_string()
  }

  pub fn char_count(&self) -> usize {
    self.spans.iter().map(|s| s.text.chars().count()).sum()
  }

  fn visible_spans(&self) -> impl Iterator<Item = &Span> {
    self.spans.iter().filter(|s| !s.text.trim().is_empty())
  }

  pub fn is_bold(&self) -> bool {
    self.visible_spans().all(|s| s.style.bold) && self.visible_spans().next().is_some()
  }

  pub fn is_mono(&self) -> bool {
    self.visible_spans().all(|s| s.style.mono) && self.visible_spans().next().is_some()
  }

  /// Trecho menor e acima (sobrescrito) ou abaixo (subscrito) da linha.
  pub fn script(&self, span: &Span) -> Script {
    if span.size >= self.size * 0.85 {
      Script::Normal
    } else if span.baseline < self.baseline - self.size * 0.2 {
      Script::Super
    } else if span.baseline > self.baseline + self.size * 0.1 {
      Script::Sub
    } else {
      Script::Normal
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
  Normal,
  Super,
  Sub,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GraphicKind {
  /// Caminho vetorial (linhas, retângulos, curvas).
  Path { filled: bool },
  /// Imagem raster, índice em [`crate::extract::Extracted::assets`].
  Image { asset: usize },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Graphic {
  pub kind: GraphicKind,
  pub bbox: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
  /// Número da página, começando em 1.
  pub number: u32,
  pub width: f32,
  pub height: f32,
  pub lines: Vec<Line>,
  pub graphics: Vec<Graphic>,
}

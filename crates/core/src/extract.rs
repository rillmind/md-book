//! Extração com o PDFium: linhas de texto com estilo e posição, traços vetoriais e imagens.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;

use pdfium_render::prelude::*;

use crate::model::{Graphic, GraphicKind, Line, Page, Rect, Span, Style};

/// Imagem raster gravada no diretório temporário de recursos.
#[derive(Debug, Clone)]
pub struct ImageAsset {
  pub file: String,
  /// Páginas em que a imagem aparece (mesmos bytes = mesma imagem).
  pub pages: Vec<u32>,
}

pub struct Extracted {
  pub pages: Vec<Page>,
  pub assets: Vec<ImageAsset>,
  /// Páginas que o PDFium não conseguiu ler.
  pub failed: Vec<u32>,
}

/// Imagens menores que isso (em pixels) são ícones ou ruído.
const MIN_IMAGE_PX: i32 = 16;
/// Profundidade máxima de XObjects aninhados.
const MAX_FORM_DEPTH: usize = 8;

pub fn extract(
  document: &PdfDocument,
  assets_dir: &Path,
  mut on_page: impl FnMut(u32, u32),
) -> Extracted {
  let total = document.pages().len() as u32;
  let mut out = Extracted {
    pages: Vec::with_capacity(total as usize),
    assets: Vec::new(),
    failed: Vec::new(),
  };
  let mut by_hash: HashMap<u64, usize> = HashMap::new();

  for (index, page) in document.pages().iter().enumerate() {
    let number = index as u32 + 1;
    let width = page.width().value;
    let height = page.height().value;
    let origin = Origin::of(&page);
    let lines = match page.text() {
      Ok(text) => extract_lines(&text, origin),
      Err(_) => {
        out.failed.push(number);
        Vec::new()
      }
    };
    let mut graphics = Vec::new();
    let mut ctx = GraphicsCtx {
      document,
      origin,
      number,
      assets_dir,
      assets: &mut out.assets,
      by_hash: &mut by_hash,
    };
    for object in page.objects().iter() {
      ctx.collect(&object, &mut graphics, 0);
    }
    // Marcas de corte e registro da gráfica ficam fora da área visível.
    graphics.retain(|g: &Graphic| {
      g.bbox.x1 > 0.0 && g.bbox.y1 > 0.0 && g.bbox.x0 < width && g.bbox.y0 < height
    });
    out.pages.push(Page {
      number,
      width,
      height,
      lines,
      graphics,
    });
    on_page(number, total);
  }
  out
}

/// Canto superior esquerdo da área visível da página (CropBox, ou MediaBox): livros de gráfica
/// têm sangria e marcas de corte, e a renderização começa nesse canto, não na origem do PDF.
#[derive(Clone, Copy)]
struct Origin {
  left: f32,
  top: f32,
}

impl Origin {
  fn of(page: &PdfPage) -> Self {
    let boundaries = page.boundaries();
    match boundaries.crop().or_else(|_| boundaries.media()) {
      Ok(b) => Self {
        left: b.bounds.left().value,
        top: b.bounds.top().value,
      },
      Err(_) => Self {
        left: 0.0,
        top: page.height().value,
      },
    }
  }

  fn rect(&self, left: f32, top: f32, right: f32, bottom: f32) -> Rect {
    Rect::new(
      left - self.left,
      self.top - top,
      right - self.left,
      self.top - bottom,
    )
  }

  fn y(&self, y: f32) -> f32 {
    self.top - y
  }
}

fn extract_lines(text: &PdfPageText, origin: Origin) -> Vec<Line> {
  let mut builder = LineBuilder::default();
  let mut fonts = FontCache::default();
  for ch in text.chars().iter() {
    let Some(c) = ch.unicode_char() else { continue };
    match c {
      '\n' | '\r' => {
        builder.line_break();
        continue;
      }
      // O PDFium usa 0x02 para hífen de fim de linha.
      '\u{2}' => {
        builder.soft_hyphen();
        continue;
      }
      c if c.is_whitespace() => {
        builder.space();
        continue;
      }
      c if c.is_control() || is_invisible(c) => continue,
      _ => {}
    }
    // Texto girado (marcas d'água, legendas verticais) não faz parte do fluxo.
    if ch
      .angle_degrees()
      .is_ok_and(|a| a.abs() > 1.0 && (a - 360.0).abs() > 1.0)
    {
      continue;
    }
    let Ok(bounds) = ch.loose_bounds() else {
      continue;
    };
    let rect = origin.rect(
      bounds.left().value,
      bounds.top().value,
      bounds.right().value,
      bounds.bottom().value,
    );
    let mut size = ch.scaled_font_size().value;
    if size <= 0.0 {
      size = rect.height();
    }
    let baseline = ch.origin_y().map(|y| origin.y(y.value)).unwrap_or(rect.y1);
    let style = fonts.style(&ch);
    builder.push(c, rect, size, baseline, style);
  }
  builder.end_line();
  dedupe(builder.lines)
}

/// Texto impresso duas vezes no mesmo lugar (negrito simulado, sombra) aparece em dobro.
fn dedupe(lines: Vec<Line>) -> Vec<Line> {
  let mut seen = std::collections::HashSet::new();
  lines
    .into_iter()
    .filter(|l| {
      let b = l.bbox;
      seen.insert((l.text(), [b.x0, b.y0, b.x1, b.y1].map(|v| v.round() as i32)))
    })
    .collect()
}

fn is_invisible(c: char) -> bool {
  matches!(
    c,
    '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
  )
}

/// Expande ligaduras tipográficas em letras comuns.
fn expand_ligature(c: char) -> Option<&'static str> {
  Some(match c {
    'ﬀ' => "ff",
    'ﬁ' => "fi",
    'ﬂ' => "fl",
    'ﬃ' => "ffi",
    'ﬄ' => "ffl",
    'ﬅ' | 'ﬆ' => "st",
    '\u{2010}' | '\u{2011}' => "-",
    _ => return None,
  })
}

/// Estilo por fonte, para não refazer a análise do nome a cada caractere.
#[derive(Default)]
struct FontCache {
  styles: HashMap<(String, u32, bool, bool, bool), Style>,
}

impl FontCache {
  fn style(&mut self, ch: &PdfPageTextChar) -> Style {
    let weight = match ch.font_weight() {
      Some(PdfFontWeight::Weight100) => 100,
      Some(PdfFontWeight::Weight200) => 200,
      Some(PdfFontWeight::Weight300) => 300,
      Some(PdfFontWeight::Weight400Normal) => 400,
      Some(PdfFontWeight::Weight500) => 500,
      Some(PdfFontWeight::Weight600) => 600,
      Some(PdfFontWeight::Weight700Bold) => 700,
      Some(PdfFontWeight::Weight800) => 800,
      Some(PdfFontWeight::Weight900) => 900,
      Some(PdfFontWeight::Custom(w)) => w,
      None => 0,
    };
    // O nome distingue fontes com os mesmos atributos (ex.: Times vs Times-Bold sem peso).
    let key = (
      ch.font_name(),
      weight,
      ch.font_is_italic(),
      ch.font_is_fixed_pitch(),
      ch.font_is_bold_reenforced(),
    );
    if let Some(style) = self.styles.get(&key) {
      return *style;
    }
    let style = style_from(&key.0, key.1, key.2, key.3, key.4);
    self.styles.insert(key, style);
    style
  }
}

fn style_from(
  name: &str,
  weight: u32,
  italic_flag: bool,
  fixed_pitch: bool,
  fake_bold: bool,
) -> Style {
  // Remove o prefixo de subconjunto ("ABCDEF+Minion-Bold").
  let base = name.rsplit('+').next().unwrap_or(name).to_lowercase();
  let has = |words: &[&str]| words.iter().any(|w| base.contains(w));
  let bold = fake_bold
    || weight >= 600
    || has(&[
      "bold", "black", "heavy", "semibold", "demi", "extrab", "ultrab",
    ])
    || base.ends_with("-bd")
    || base.ends_with("-b")
    || is_tex_bold(&base);
  let italic = italic_flag
    || has(&["italic", "oblique", "-it", "ital", "slant"])
    || base.starts_with("cmti")
    || base.starts_with("cmsl");
  let mono = fixed_pitch
    || has(&[
      "mono",
      "courier",
      "consol",
      "menlo",
      "inconsolata",
      "typewriter",
      "code",
    ])
    || is_tex_mono(&base);
  Style { bold, italic, mono }
}

/// Fontes Computer Modern / Latin Modern: cmbx10, lmroman10-bold, sfbx...
fn is_tex_bold(name: &str) -> bool {
  name.starts_with("cmbx") || name.starts_with("cmb10") || name.starts_with("sfbx")
}

fn is_tex_mono(name: &str) -> bool {
  name.starts_with("cmtt")
    || name.starts_with("sftt")
    || name.starts_with("lmmono")
    || name.contains("tt10")
}

#[derive(Default)]
struct LineBuilder {
  lines: Vec<Line>,
  spans: Vec<Span>,
  pending_space: bool,
  /// O PDFium indicou fim de linha; confirmado só se o próximo caractere estiver noutra linha
  /// (ele também quebra antes de expoentes e índices).
  pending_break: bool,
  last_x1: f32,
  /// Tamanho e linha de base de referência da linha atual (primeiro caractere de tamanho normal).
  line_size: f32,
  line_baseline: f32,
}

impl LineBuilder {
  fn line_break(&mut self) {
    if !self.spans.is_empty() {
      self.pending_break = true;
    }
  }

  fn space(&mut self) {
    if !self.spans.is_empty() {
      self.pending_space = true;
    }
  }

  fn soft_hyphen(&mut self) {
    if let Some(span) = self.spans.last_mut() {
      span.text.push('\u{ad}');
    }
  }

  fn push(&mut self, c: char, rect: Rect, size: f32, baseline: f32, style: Style) {
    if !self.spans.is_empty() {
      let dy = (baseline - self.line_baseline).abs();
      // Expoente ou índice: menor e deslocado verticalmente, mas na mesma linha.
      let script = size < self.line_size * 0.85 && dy < self.line_size;
      let same_line = if self.pending_break {
        (script || dy < self.line_size * 0.3) && rect.x0 >= self.last_x1 - size * 0.5
      } else {
        script || dy <= self.line_size.max(size) * 0.6
      };
      // Voltar muito para a esquerda é início de outra linha (ou coluna).
      let went_back = rect.x0 < self.last_x1 - size * 2.0;
      if !same_line || went_back {
        self.end_line();
      }
    }
    self.pending_break = false;
    if self.spans.is_empty() {
      self.line_size = size;
      self.line_baseline = baseline;
    } else if size > self.line_size * 1.15 && (baseline - self.line_baseline).abs() < size * 0.3 {
      // Uma capitular ou palavra maior na mesma linha passa a ser a referência.
      self.line_size = size;
      self.line_baseline = baseline;
    }

    let gap = rect.x0 - self.last_x1;
    // Capitular (letra inicial grande) emenda na palavra sem espaço: "U" + "m computador".
    let after_drop_cap = self.spans.len() == 1
      && self.spans[0].text.chars().count() == 1
      && self.spans[0].size >= size * 1.8;
    let space_before =
      !after_drop_cap && (self.pending_space || (!self.spans.is_empty() && gap > size * 0.5));
    let new_span = match self.spans.last() {
      None => true,
      Some(span) => {
        span.style != style
          || (span.size - size).abs() > span.size * 0.1
          || (span.baseline - baseline).abs() > size * 0.15
          || gap > size * 1.2
      }
    };
    if new_span {
      self.spans.push(Span {
        text: String::new(),
        bbox: rect,
        size,
        baseline,
        style,
        space_before,
      });
    } else if space_before {
      self.spans.last_mut().unwrap().text.push(' ');
    }
    let span = self.spans.last_mut().unwrap();
    match expand_ligature(c) {
      Some(s) => span.text.push_str(s),
      None => span.text.push(c),
    }
    span.bbox = span.bbox.union(&rect);
    self.pending_space = false;
    self.last_x1 = rect.x1;
  }

  fn end_line(&mut self) {
    self.pending_space = false;
    self.pending_break = false;
    self.last_x1 = 0.0;
    let mut spans = std::mem::take(&mut self.spans);
    spans.retain(|s| !s.text.trim().is_empty() || s.text.contains('\u{ad}'));
    if spans.is_empty() {
      return;
    }
    let bbox = spans
      .iter()
      .skip(1)
      .fold(spans[0].bbox, |acc, s| acc.union(&s.bbox));
    let mut by_size: HashMap<u32, usize> = HashMap::new();
    for span in &spans {
      *by_size.entry((span.size * 2.0).round() as u32).or_default() += span.text.chars().count();
    }
    let size = by_size
      .into_iter()
      .max_by_key(|&(size, count)| (count, size))
      .map(|(size, _)| size as f32 / 2.0)
      .unwrap_or(self.line_size);
    // Linha de base do texto de tamanho dominante (não a de uma capitular ou expoente).
    let baseline = spans
      .iter()
      .find(|s| ((s.size * 2.0).round() / 2.0 - size).abs() < 0.3)
      .map_or(self.line_baseline, |s| s.baseline);
    self.lines.push(Line {
      spans,
      bbox,
      size,
      baseline,
    });
  }
}

struct GraphicsCtx<'a, 'd> {
  document: &'a PdfDocument<'d>,
  origin: Origin,
  number: u32,
  assets_dir: &'a Path,
  assets: &'a mut Vec<ImageAsset>,
  by_hash: &'a mut HashMap<u64, usize>,
}

impl GraphicsCtx<'_, '_> {
  fn rect(&self, object: &PdfPageObject) -> Option<Rect> {
    let b = object.bounds().ok()?;
    Some(self.origin.rect(
      b.left().value,
      b.top().value,
      b.right().value,
      b.bottom().value,
    ))
  }

  fn collect(&mut self, object: &PdfPageObject, out: &mut Vec<Graphic>, depth: usize) {
    match object {
      PdfPageObject::Path(path) => {
        let Some(bbox) = self.rect(object) else {
          return;
        };
        let filled = path.fill_mode().is_ok_and(|m| m != PdfPathFillMode::None);
        out.push(Graphic {
          kind: GraphicKind::Path { filled },
          bbox,
        });
      }
      PdfPageObject::Image(image) => {
        let Some(bbox) = self.rect(object) else {
          return;
        };
        if let Some(asset) = self.save_image(image) {
          out.push(Graphic {
            kind: GraphicKind::Image { asset },
            bbox,
          });
        }
      }
      PdfPageObject::XObjectForm(form) if depth < MAX_FORM_DEPTH => {
        for child in form.iter() {
          self.collect(&child, out, depth + 1);
        }
      }
      _ => {}
    }
  }

  /// Grava a imagem uma única vez por conteúdo; JPEG é copiado sem recodificar.
  fn save_image(&mut self, image: &PdfPageImageObject) -> Option<usize> {
    let (w, h) = (image.width().ok()?, image.height().ok()?);
    if w < MIN_IMAGE_PX || h < MIN_IMAGE_PX {
      return None;
    }
    let raw = image.get_raw_image_data().ok()?;
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    (w, h).hash(&mut hasher);
    let hash = hasher.finish();

    if let Some(&id) = self.by_hash.get(&hash) {
      let pages = &mut self.assets[id].pages;
      if pages.last() != Some(&self.number) {
        pages.push(self.number);
      }
      return Some(id);
    }

    let filters = image.filters();
    let only_dct = filters.len() == 1
      && filters
        .get(0)
        .is_ok_and(|f| f.name() == "DCTDecode" || f.name() == "DCT");
    let id = self.assets.len();
    let file = if only_dct {
      let file = format!("img-{id:04}.jpg");
      std::fs::write(self.assets_dir.join(&file), &raw).ok()?;
      file
    } else {
      let file = format!("img-{id:04}.png");
      let decoded = image.get_processed_image(self.document).ok()?;
      decoded
        .save_with_format(self.assets_dir.join(&file), image::ImageFormat::Png)
        .ok()?;
      file
    };
    self.by_hash.insert(hash, id);
    self.assets.push(ImageAsset {
      file,
      pages: vec![self.number],
    });
    Some(id)
  }
}

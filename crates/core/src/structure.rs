//! Transforma as linhas de cada página em estrutura: títulos, parágrafos, listas e capítulos.

use std::collections::{HashMap, HashSet};

use crate::layout::PageContent;
use crate::model::{Line, Style};
use crate::outline::OutlineEntry;
use crate::text::{self, ListKind, Vocabulary};

/// Trecho de texto dentro de um parágrafo.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
  Text {
    text: String,
    style: Style,
  },
  /// Quebra de linha dentro do parágrafo (versos).
  Break,
}

impl Inline {
  pub fn plain(text: impl Into<String>) -> Self {
    Inline::Text {
      text: text.into(),
      style: Style::default(),
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
  Heading {
    level: u8,
    text: String,
  },
  Paragraph(Vec<Inline>),
  ListItem {
    kind: ListKind,
    /// Nível de aninhamento, começando em 0.
    depth: u8,
    content: Vec<Inline>,
  },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
  pub title: String,
  /// Profundidade no SUMMARY.md (1 = capítulo de topo).
  pub depth: u8,
  /// Conteúdo, sem o título do capítulo.
  pub blocks: Vec<Block>,
}

/// De onde vieram os títulos detectados.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadingSource {
  Outline,
  FontSize,
  None,
}

pub struct Structured {
  pub blocks: Vec<Block>,
  pub source: HeadingSource,
  /// O PDF tinha outline, mas ele deixava de fora títulos principais e foi descartado.
  pub outline_ignored: bool,
}

/// Título dado ao conteúdo que aparece antes do primeiro capítulo detectado.
pub const FRONT_MATTER_TITLE: &str = "Início";

/// Tamanho a partir do qual uma linha é considerada maior que o corpo do texto.
const HEADING_SIZE_RATIO: f32 = 1.15;
/// Linhas mais longas que isso nunca são títulos.
const MAX_HEADING_CHARS: usize = 120;
/// Níveis de título distintos detectados por tamanho de fonte.
const MAX_FONT_LEVELS: u8 = 3;
/// Espaço vertical (relativo ao entrelinha mediano) que separa parágrafos.
const PARAGRAPH_GAP_RATIO: f32 = 1.3;

enum Item<'a> {
  Heading {
    level: u8,
    text: String,
    /// Tamanho da linha de origem (ausente para entradas do outline sem linha correspondente).
    size: Option<f32>,
  },
  Line(&'a Line),
}

/// Monta os blocos do livro inteiro, usando o outline quando houver e parecer completo.
pub fn build_blocks(pages: &[PageContent], outline: &[OutlineEntry]) -> Structured {
  let fonts = FontLevels::new(pages);
  let mut vocabulary = Vocabulary::default();
  for line in pages.iter().flat_map(PageContent::lines) {
    vocabulary.add_line(&line.text());
  }
  let font: Vec<Vec<Item>> = pages.iter().map(|p| font_items(p, &fonts)).collect();

  if !outline.is_empty() {
    let placed = place_outline(pages, outline, &fonts);
    let ignored = outline_misses_chapters(pages, outline, &font, placed.max_matched_size);
    let (items, source) = if ignored {
      (&font, HeadingSource::FontSize)
    } else {
      (&placed.items, HeadingSource::Outline)
    };
    return Structured {
      blocks: assemble(pages, items, &fonts, &vocabulary),
      source,
      outline_ignored: ignored,
    };
  }

  let found = font
    .iter()
    .flatten()
    .any(|i| matches!(i, Item::Heading { .. }));
  Structured {
    blocks: assemble(pages, &font, &fonts, &vocabulary),
    source: if found {
      HeadingSource::FontSize
    } else {
      HeadingSource::None
    },
    outline_ignored: false,
  }
}

fn size_key(size: f32) -> u32 {
  (size * 2.0).round() as u32
}

/// Hierarquia de títulos pelo tamanho de fonte.
struct FontLevels {
  body: f32,
  /// Tamanho (em meio-pontos) → nível de título.
  levels: HashMap<u32, u8>,
}

impl FontLevels {
  fn new(pages: &[PageContent]) -> Self {
    let mut chars_by_size: HashMap<u32, usize> = HashMap::new();
    let mut pages_by_size: HashMap<u32, HashSet<u32>> = HashMap::new();
    for page in pages {
      for line in page.lines().filter(|l| !l.is_mono()) {
        let key = size_key(line.size);
        *chars_by_size.entry(key).or_default() += line.char_count();
        pages_by_size.entry(key).or_default().insert(page.number);
      }
    }
    let Some((&body, &body_chars)) = chars_by_size.iter().max_by_key(|&(k, n)| (*n, *k)) else {
      return Self {
        body: 0.0,
        levels: HashMap::new(),
      };
    };

    // Tamanhos maiores que o corpo e raros o bastante para serem títulos.
    let mut candidates: Vec<u32> = chars_by_size
      .iter()
      .filter(|&(&k, &n)| k as f32 > body as f32 * HEADING_SIZE_RATIO && n * 5 < body_chars)
      .map(|(&k, _)| k)
      .collect();
    candidates.sort_unstable_by(|a, b| b.cmp(a));

    // Tamanhos que aparecem numa única página (ex.: capa) não definem a hierarquia.
    let mut ranked: Vec<u32> = candidates
      .iter()
      .copied()
      .filter(|k| pages_by_size[k].len() >= 2)
      .collect();
    if ranked.is_empty() {
      ranked = candidates.clone();
    }

    let levels = candidates
      .into_iter()
      .map(|k| {
        let above = ranked.iter().filter(|&&r| r > k).count() as u8;
        (k, (above + 1).min(MAX_FONT_LEVELS))
      })
      .collect();
    Self {
      body: body as f32 / 2.0,
      levels,
    }
  }

  fn deepest(&self) -> u8 {
    self.levels.values().copied().max().unwrap_or(0)
  }

  /// Nível de título da linha pelo tamanho, ou por negrito no tamanho do corpo.
  fn level(&self, line: &Line, prev: Option<&Line>, next: Option<&Line>) -> Option<u8> {
    let text = line.text();
    let letters = text.chars().filter(|c| c.is_alphanumeric()).count();
    if line.is_mono() || text.chars().count() > MAX_HEADING_CHARS || letters < 1 {
      return None;
    }
    if let Some(&level) = self.levels.get(&size_key(line.size)) {
      return Some(level);
    }
    // Negrito do tamanho do corpo, sozinho entre linhas que não são negrito: subtítulo.
    let bold_alone = line.is_bold()
      && line.size >= self.body * 0.95
      && letters >= 2
      && !text.trim_end().ends_with(['.', ',', ';'])
      && text::list_marker(&text).is_none()
      && prev.is_none_or(|p| !p.is_bold())
      && next.is_none_or(|n| !n.is_bold());
    bold_alone.then(|| (self.deepest() + 1).min(6))
  }
}

fn font_items<'a>(page: &'a PageContent, fonts: &FontLevels) -> Vec<Item<'a>> {
  let lines: Vec<&Line> = page.lines().collect();
  let mut items: Vec<Item> = Vec::with_capacity(lines.len());
  let mut last_heading: Option<&Line> = None;
  for (i, &line) in lines.iter().enumerate() {
    let prev = i.checked_sub(1).map(|j| lines[j]);
    let next = lines.get(i + 1).copied();
    let Some(level) = fonts.level(line, prev, next) else {
      items.push(Item::Line(line));
      last_heading = None;
      continue;
    };
    let text = line.text();
    let close_below =
      last_heading.is_some_and(|h| line.baseline - h.baseline < line.size.max(h.size) * 2.0);
    match items.last_mut() {
      // Título quebrado em várias linhas, ou número do capítulo numa linha à parte.
      Some(Item::Heading {
        level: prev_level,
        text: prev_text,
        size,
      }) if close_below
        && (*prev_level == level || !prev_text.chars().any(char::is_alphabetic)) =>
      {
        prev_text.push(' ');
        prev_text.push_str(&text);
        *prev_level = (*prev_level).min(level);
        *size = Some(size.unwrap_or(0.0).max(line.size));
      }
      _ => items.push(Item::Heading {
        level,
        text,
        size: Some(line.size),
      }),
    }
    last_heading = Some(line);
  }
  items
}

struct PlacedOutline<'a> {
  items: Vec<Vec<Item<'a>>>,
  /// Maior tamanho de fonte entre as linhas que corresponderam a entradas do outline.
  max_matched_size: Option<f32>,
}

/// Coloca cada entrada do outline na linha correspondente da sua página. Linhas que estão no
/// texto só como parte do título do bookmark ("5" + "HOW THE KERNEL BOOTS") são consumidas.
/// Títulos menores que os do outline continuam sendo detectados como subtítulos.
fn place_outline<'a>(
  pages: &'a [PageContent],
  outline: &[OutlineEntry],
  fonts: &FontLevels,
) -> PlacedOutline<'a> {
  let max_outline_level = outline.iter().map(|e| e.level).max().unwrap_or(1);
  let prominent = |line: &Line| line.size >= fonts.body * 1.05 || line.is_bold();
  let mut matched_sizes: Vec<f32> = Vec::new();
  let mut placements_per_page = Vec::with_capacity(pages.len());

  for page in pages {
    let lines: Vec<&Line> = page.lines().collect();
    let mut placements: Vec<(usize, usize, &OutlineEntry)> = Vec::new();
    let mut cursor = 0;
    for entry in outline.iter().filter(|e| e.page == page.number) {
      let target = text::normalize(&entry.title);
      let matches = |line: &Line| {
        let key = text::normalize(&line.text());
        if key.is_empty() {
          return false;
        }
        let part =
          target.contains(&key) && (key.len() >= 3 || key.chars().all(|c| c.is_ascii_digit()));
        key == target || (prominent(line) && (key.starts_with(&target) || part))
      };
      let (start, end) = match (cursor..lines.len()).find(|&i| matches(lines[i])) {
        Some(start) => {
          let mut end = start + 1;
          while end < lines.len() && matches(lines[end]) && prominent(lines[end]) {
            end += 1;
          }
          matched_sizes.extend(lines[start..end].iter().map(|l| l.size));
          (start, end)
        }
        None => (cursor, cursor),
      };
      placements.push((start, end, entry));
      cursor = end;
    }
    placements_per_page.push(placements);
  }

  let max_matched_size = matched_sizes.iter().copied().reduce(f32::max);
  let min_matched_size = matched_sizes.iter().copied().reduce(f32::min);
  let mut items = Vec::with_capacity(pages.len());

  for (page, placements) in pages.iter().zip(placements_per_page) {
    let lines: Vec<&Line> = page.lines().collect();
    let mut page_items = Vec::with_capacity(lines.len() + placements.len());
    let mut next = placements.iter().peekable();
    // Linhas que formam o título já viraram o Heading e não se repetem como texto.
    let mut skip_to = 0;
    for (i, &line) in lines.iter().enumerate() {
      while let Some(&&(start, end, entry)) = next.peek() {
        if start != i {
          break;
        }
        page_items.push(Item::Heading {
          level: entry.level,
          text: entry.title.clone(),
          size: None,
        });
        skip_to = skip_to.max(end);
        next.next();
      }
      if i < skip_to {
        continue;
      }
      let prev = i.checked_sub(1).map(|j| lines[j]);
      let sub = min_matched_size.and_then(|min| {
        fonts.level(line, prev, lines.get(i + 1).copied())?;
        let bold_body = !fonts.levels.contains_key(&size_key(line.size));
        let extra = if bold_body {
          2
        } else if line.size < min - 0.5 {
          1
        } else {
          return None;
        };
        Some((max_outline_level + extra).min(6))
      });
      page_items.push(match sub {
        Some(level) => Item::Heading {
          level,
          text: line.text(),
          size: Some(line.size),
        },
        None => Item::Line(line),
      });
    }
    // Entradas não localizadas no fim da página (ou página sem texto).
    for &(_, _, entry) in next {
      page_items.push(Item::Heading {
        level: entry.level,
        text: entry.title.clone(),
        size: None,
      });
    }
    items.push(page_items);
  }

  PlacedOutline {
    items,
    max_matched_size,
  }
}

/// Outlines incompletos são comuns (ex.: exportadores que só marcam subtítulos). O outline é
/// descartado quando há títulos *maiores* que os dele, fora dele, espalhados por várias seções.
/// Títulos grandes concentrados num só trecho (ex.: anúncios no fim do livro) não contam.
fn outline_misses_chapters(
  pages: &[PageContent],
  outline: &[OutlineEntry],
  font: &[Vec<Item>],
  max_matched_size: Option<f32>,
) -> bool {
  let Some(max_matched) = max_matched_size else {
    return false;
  };
  let titles: Vec<String> = outline
    .iter()
    .map(|e| text::normalize(&e.title))
    .filter(|t| !t.is_empty())
    .collect();
  let mut sections = HashSet::new();
  let mut missing = 0;
  for (page, items) in pages.iter().zip(font) {
    for item in items {
      let Item::Heading {
        level: 1,
        text,
        size: Some(size),
      } = item
      else {
        continue;
      };
      let key = text::normalize(text);
      let known = titles
        .iter()
        .any(|t| t.contains(&key) || key.contains(t.as_str()));
      if key.is_empty() || known || *size <= max_matched + 0.5 {
        continue;
      }
      missing += 1;
      sections.insert(outline.iter().filter(|e| e.page <= page.number).count());
    }
  }
  missing >= 2 && sections.len() >= 2
}

/// Medidas de uma página para decidir quebras de parágrafo. Margens são calculadas por coluna:
/// para cada linha, olhando só as linhas do corpo que ocupam a mesma faixa horizontal.
struct PageStats {
  /// Extensão horizontal (x0, x1) e tamanho das linhas do corpo do texto.
  body: Vec<(f32, f32, f32)>,
  /// Entrelinha mediano do corpo do texto.
  gap: Option<f32>,
  /// Página em verso (poesia, teatro): as quebras de linha fazem parte do texto.
  verse: bool,
}

/// Fração de pares de linhas (sem pontuação final) em que a próxima palavra caberia na linha
/// anterior, acima da qual a página é verso: em prosa a linha só quebra quando a palavra não cabe.
const VERSE_FIT_RATIO: f32 = 0.6;

impl PageStats {
  fn new(lines: &[&Line], body: f32) -> Self {
    // Código e títulos não dizem nada sobre o alinhamento do texto corrido.
    let source: Vec<&Line> = lines
      .iter()
      .copied()
      .filter(|l| (l.size - body).abs() <= body * 0.1 && !l.is_mono())
      .collect();
    let mut gaps: Vec<f32> = source
      .windows(2)
      .map(|w| w[1].baseline - w[0].baseline)
      .filter(|g| *g > 0.0 && *g < body.max(1.0) * 3.0)
      .collect();
    gaps.sort_by(f32::total_cmp);
    let mut stats = Self {
      body: source
        .iter()
        .map(|l| (l.bbox.x0, l.bbox.x1, l.size))
        .collect(),
      gap: gaps.get(gaps.len() / 2).copied(),
      verse: false,
    };

    // Numeração de linha na margem de 5 em 5 é marca de verso (teatro, poesia).
    let numbers: Vec<u32> = lines.iter().filter_map(|l| margin_number(l)).collect();
    let numbered = numbers.len() >= 2 && numbers.iter().all(|n| n % 5 == 0);
    let (mut considered, mut fits) = (0, 0);
    for pair in source.windows(2) {
      let (a, b) = (pair[0], pair[1]);
      let delta = b.baseline - a.baseline;
      let consecutive = delta > 0.0 && stats.gap.is_none_or(|g| delta <= g * 1.5);
      if !consecutive
        || !overlaps(a, b)
        || !is_prose(a)
        || !is_prose(b)
        || text::ends_sentence(&a.text())
      {
        continue;
      }
      considered += 1;
      if first_word_fits(b, a, stats.column(a).1) {
        fits += 1;
      }
    }
    stats.verse =
      numbered || (considered >= 5 && fits as f32 >= considered as f32 * VERSE_FIT_RATIO);
    stats
  }

  /// Margens esquerda (mais comum) e direita (quase máxima) da coluna da linha: linhas que se
  /// sobrepõem a ela e começam na mesma margem (com folga para o recuo de parágrafo).
  fn column(&self, line: &Line) -> (f32, f32) {
    let same: Vec<&(f32, f32, f32)> = self
      .body
      .iter()
      .filter(|&&(x0, x1, size)| {
        let overlap = x1.min(line.bbox.x1) - x0.max(line.bbox.x0);
        overlap >= (x1 - x0).min(line.bbox.width()) * 0.5 && (x0 - line.bbox.x0).abs() <= size * 3.0
      })
      .collect();
    if same.is_empty() {
      return (line.bbox.x0, line.bbox.x1);
    }
    let mut lefts: HashMap<i32, usize> = HashMap::new();
    for (x0, _, _) in &same {
      *lefts.entry((x0 / 2.0).round() as i32).or_default() += 1;
    }
    let left = lefts
      .into_iter()
      .max_by_key(|&(x, n)| (n, -x))
      .map_or(line.bbox.x0, |(x, _)| x as f32 * 2.0);
    let mut rights: Vec<f32> = same.iter().map(|(_, x1, _)| *x1).collect();
    rights.sort_by(f32::total_cmp);
    let right = rights[(rights.len() - 1) * 19 / 20];
    (left, right)
  }

  fn indented(&self, line: &Line) -> bool {
    let indent = line.bbox.x0 - self.column(line).0;
    indent > line.size * 0.8 && indent < line.size * 6.0
  }

  fn right(&self, line: &Line) -> f32 {
    self.column(line).1
  }
}

/// Linha de texto corrido: várias palavras, maioria letras (não fórmula nem tabela de números).
fn is_prose(line: &Line) -> bool {
  let text = line.text();
  let letters = text.chars().filter(|c| c.is_alphabetic()).count();
  text.split_whitespace().count() >= 2
    && letters * 2 >= text.chars().filter(|c| !c.is_whitespace()).count()
}

fn overlaps(a: &Line, b: &Line) -> bool {
  a.bbox.x1.min(b.bbox.x1) - a.bbox.x0.max(b.bbox.x0) > a.bbox.width().min(b.bbox.width()) * 0.5
}

enum Current {
  Paragraph(Vec<Inline>),
  Item {
    kind: ListKind,
    depth: u8,
    content: Vec<Inline>,
    /// Onde começa o texto do item (depois do marcador): linhas de continuação alinham aqui.
    text_x0: f32,
  },
}

impl Current {
  fn inlines(&self) -> &[Inline] {
    match self {
      Current::Paragraph(content) | Current::Item { content, .. } => content,
    }
  }

  fn inlines_mut(&mut self) -> &mut Vec<Inline> {
    match self {
      Current::Paragraph(content) | Current::Item { content, .. } => content,
    }
  }

  fn last_text(&self) -> &str {
    match self.inlines().last() {
      Some(Inline::Text { text, .. }) => text,
      _ => "",
    }
  }

  fn ends_sentence(&self) -> bool {
    text::ends_sentence(self.last_text())
  }

  fn into_block(self) -> Block {
    match self {
      Current::Paragraph(content) => Block::Paragraph(content),
      Current::Item {
        kind,
        depth,
        content,
        ..
      } => Block::ListItem {
        kind,
        depth,
        content,
      },
    }
  }
}

struct Assembler<'v> {
  blocks: Vec<Block>,
  current: Option<Current>,
  /// Posição do marcador do primeiro item da lista atual, para calcular aninhamento.
  list_base_x: Option<f32>,
  vocabulary: &'v Vocabulary,
}

impl Assembler<'_> {
  fn close(&mut self) {
    if let Some(current) = self.current.take() {
      let empty = current.inlines().iter().all(|i| match i {
        Inline::Text { text, .. } => text.trim().is_empty(),
        Inline::Break => true,
      });
      if !empty {
        self.blocks.push(current.into_block());
      }
    }
  }

  fn heading(&mut self, level: u8, text: &str) {
    self.close();
    self.list_base_x = None;
    self.blocks.push(Block::Heading {
      level,
      text: text.trim().to_string(),
    });
  }

  fn page_start(&mut self) {
    if self.current.as_ref().is_some_and(Current::ends_sentence) {
      self.close();
    }
  }

  fn line(&mut self, line: &Line, prev: Option<&Line>, stats: &PageStats) {
    let text = line.text();
    if stats.verse {
      self.verse_line(line, prev, stats);
      return;
    }
    if let Some((kind, rest)) = text::list_marker(&text) {
      self.close();
      let base = *self.list_base_x.get_or_insert(line.bbox.x0);
      let depth = if line.bbox.x0 > base {
        ((line.bbox.x0 - base) / (line.size * 1.5))
          .round()
          .clamp(0.0, 3.0) as u8
      } else {
        self.list_base_x = Some(line.bbox.x0);
        0
      };
      let marker_share = (text.len() - rest.len()) as f32 / text.len().max(1) as f32;
      self.current = Some(Current::Item {
        kind,
        depth,
        content: line_inlines(line, Some(rest), stats),
        text_x0: line.bbox.x0 + line.bbox.width() * marker_share,
      });
      return;
    }

    let wide_gap = prev.is_some_and(|p| {
      let delta = line.baseline - p.baseline;
      delta < -p.size || stats.gap.is_some_and(|g| delta > g * PARAGRAPH_GAP_RATIO)
    });
    // Linha alinhada ao texto do item (recuo deslocado) continua o item.
    let continues_item = match &self.current {
      Some(Current::Item { text_x0, .. }) => !wide_gap && line.bbox.x0 >= text_x0 - line.size,
      _ => false,
    };
    let continues = self
      .current
      .as_ref()
      .is_some_and(|c| text::continues_sentence(c.last_text(), &text));
    let breaks = !continues_item
      && !continues
      && match prev {
        None => self.current.is_none() || stats.indented(line),
        Some(p) => {
          let indent = line.bbox.x0 - p.bbox.x0 > line.size * 0.8 && stats.indented(line);
          let short_end =
            text::ends_sentence(&p.text()) && first_word_fits(line, p, stats.right(p));
          let size_change = (line.size - p.size).abs() > p.size.max(1.0) * 0.15;
          let was_item = matches!(self.current, Some(Current::Item { .. }));
          wide_gap || indent || short_end || size_change || was_item
        }
      };

    match self.current.as_mut() {
      Some(current) if !breaks => join_inlines(
        current.inlines_mut(),
        line_inlines(line, None, stats),
        self.vocabulary,
      ),
      _ => {
        self.close();
        self.list_base_x = None;
        self.current = Some(Current::Paragraph(line_inlines(line, None, stats)));
      }
    }
  }

  /// Em verso cada linha é preservada; estrofes se separam por espaço maior, e nomes de
  /// personagem em maiúsculas (teatro) ficam em parágrafo próprio.
  fn verse_line(&mut self, line: &Line, prev: Option<&Line>, stats: &PageStats) {
    let text = line.text();
    let speaker = is_caps_label(&text);
    let wide_gap = prev.is_some_and(|p| {
      let delta = line.baseline - p.baseline;
      delta < -p.size || stats.gap.is_some_and(|g| delta > g * PARAGRAPH_GAP_RATIO)
    });
    let after_speaker =
      matches!(&self.current, Some(c) if is_caps_label(c.last_text()) && c.inlines().len() == 1);
    let inlines = line_inlines(line, None, stats);
    match self.current.as_mut() {
      Some(Current::Paragraph(content)) if !(wide_gap || speaker || after_speaker) => {
        let last = match content.last() {
          Some(Inline::Text { text, .. }) => text.trim_end(),
          _ => "",
        };
        // Palavra hifenizada no fim da linha continua na próxima, mesmo em verso.
        let hyphenated = last.ends_with('\u{ad}')
          || (last.ends_with('-') && last.chars().rev().nth(1).is_some_and(char::is_alphabetic));
        if hyphenated {
          join_inlines(content, inlines, self.vocabulary);
        } else {
          content.push(Inline::Break);
          content.extend(inlines);
        }
      }
      _ => {
        self.close();
        self.current = Some(Current::Paragraph(inlines));
      }
    }
  }
}

/// A linha anterior terminou antes da margem mesmo havendo espaço para a primeira palavra desta:
/// a quebra foi intencional (fim de parágrafo), não do alinhamento.
fn first_word_fits(line: &Line, prev: &Line, right: f32) -> bool {
  let text = line.text();
  let char_width = line.bbox.width() / line.char_count().max(1) as f32;
  let first_word = text
    .split_whitespace()
    .next()
    .map_or(0, |w| w.chars().count());
  prev.bbox.x1 + (first_word as f32 + 1.0) * char_width < right
}

/// Converte a linha (ou o texto depois do marcador de lista) em trechos.
fn line_inlines(line: &Line, rest: Option<&str>, stats: &PageStats) -> Vec<Inline> {
  let text = match rest {
    Some(rest) => rest.to_string(),
    None if stats.verse => without_margin_numbers(line),
    None => line.text(),
  };
  vec![Inline::plain(text::strip_soft_hyphens(&text))]
}

/// Índice do trecho que é número de linha na margem: número isolado por um espaço grande no
/// começo ou no fim da linha.
fn margin_number_span(line: &Line) -> Option<usize> {
  let spans = &line.spans;
  if spans.len() < 2 {
    return None;
  }
  let number = |i: usize| {
    let t = spans[i].text.trim();
    !t.is_empty() && t.len() <= 4 && t.chars().all(|c| c.is_ascii_digit())
  };
  let far = |a: usize, b: usize| spans[b].bbox.x0 - spans[a].bbox.x1 > line.size * 1.5;
  let last = spans.len() - 1;
  if number(0) && far(0, 1) {
    Some(0)
  } else if number(last) && far(last - 1, last) {
    Some(last)
  } else {
    None
  }
}

fn margin_number(line: &Line) -> Option<u32> {
  margin_number_span(line).and_then(|i| line.spans[i].text.trim().parse().ok())
}

/// Texto da linha sem a numeração de margem ("40", "45"...).
fn without_margin_numbers(line: &Line) -> String {
  let Some(index) = margin_number_span(line) else {
    return line.text();
  };
  let mut kept = line.clone();
  kept.spans.remove(index);
  kept.text()
}

/// Rótulo todo em maiúsculas e curto, como o nome de um personagem.
fn is_caps_label(text: &str) -> bool {
  let letters: Vec<char> = text.chars().filter(|c| c.is_alphabetic()).collect();
  letters.len() >= 2 && text.chars().count() <= 40 && letters.iter().all(|c| c.is_uppercase())
}

fn join_inlines(current: &mut Vec<Inline>, next: Vec<Inline>, vocabulary: &Vocabulary) {
  let mut next = next.into_iter();
  let Some(first) = next.next() else { return };
  match (current.last_mut(), first) {
    (
      Some(Inline::Text { text, style }),
      Inline::Text {
        text: first,
        style: s,
      },
    ) if *style == s => {
      text::join(text, &first, vocabulary);
    }
    // Mudança de estilo na virada da linha: hífen invisível some, senão vira espaço.
    (Some(Inline::Text { text, .. }), first) => {
      if text.ends_with('\u{ad}') {
        text.pop();
      } else if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
      }
      current.push(first);
    }
    (_, first) => current.push(first),
  }
  current.extend(next);
}

fn assemble(
  pages: &[PageContent],
  items: &[Vec<Item>],
  fonts: &FontLevels,
  vocabulary: &Vocabulary,
) -> Vec<Block> {
  let mut asm = Assembler {
    blocks: Vec::new(),
    current: None,
    list_base_x: None,
    vocabulary,
  };
  for (page, page_items) in pages.iter().zip(items) {
    let lines: Vec<&Line> = page.lines().collect();
    let stats = PageStats::new(&lines, fonts.body);
    asm.page_start();
    let mut prev: Option<&Line> = None;
    for item in page_items {
      match item {
        Item::Heading { level, text, .. } => {
          asm.heading(*level, text);
          prev = None;
        }
        Item::Line(line) => {
          asm.line(line, prev, &stats);
          prev = Some(line);
        }
      }
    }
  }
  asm.close();
  asm.blocks
}

/// Escolhe em que nível de título quebrar arquivos: capítulos, ou o nível abaixo quando o topo
/// são poucas "partes" agrupando muitos capítulos.
pub fn auto_split_level(blocks: &[Block]) -> u8 {
  let count = |n: u8| {
    blocks
      .iter()
      .filter(|b| matches!(b, Block::Heading { level, .. } if *level == n))
      .count()
  };
  let (top, second) = (count(1), count(2));
  if top <= 6 && second >= 6 && second >= top.max(1) * 3 {
    2
  } else {
    1
  }
}

/// Divide os blocos em capítulos nos títulos de nível `<= split_level`.
pub fn split_chapters(blocks: Vec<Block>, split_level: u8) -> Vec<Chapter> {
  let mut chapters: Vec<Chapter> = Vec::new();
  for block in blocks {
    match block {
      Block::Heading { level, text } if level <= split_level => chapters.push(Chapter {
        title: text,
        depth: level,
        blocks: Vec::new(),
      }),
      block => match chapters.last_mut() {
        Some(chapter) => chapter.blocks.push(block),
        None => chapters.push(Chapter {
          title: FRONT_MATTER_TITLE.to_string(),
          depth: 1,
          blocks: vec![block],
        }),
      },
    }
  }
  // Capítulos vazios sem filhos (ex.: "Sumário", cujo conteúdo foi removido) somem.
  let keep: Vec<bool> = (0..chapters.len())
    .map(|i| {
      let has_child = chapters
        .get(i + 1)
        .is_some_and(|next| next.depth > chapters[i].depth);
      !chapters[i].blocks.is_empty() || has_child || chapters.len() == 1
    })
    .collect();
  let mut keep = keep.into_iter();
  chapters.retain(|_| keep.next().unwrap_or(true));

  // Profundidade não pode pular níveis no SUMMARY.md.
  let mut prev_depth = 0;
  for chapter in &mut chapters {
    chapter.depth = chapter.depth.min(prev_depth + 1).max(1);
    prev_depth = chapter.depth;
  }
  chapters
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::layout::Element;
  use crate::model::{Rect, Span};

  fn line_full(text: &str, size: f32, x0: f32, baseline: f32, style: Style) -> Line {
    let width = text.chars().count() as f32 * size * 0.5;
    let bbox = Rect::new(x0, baseline - size, x0 + width, baseline + size * 0.25);
    Line {
      spans: vec![Span {
        text: text.to_string(),
        bbox,
        size,
        baseline,
        style,
        space_before: false,
      }],
      bbox,
      size,
      baseline,
    }
  }

  fn line(text: &str, size: f32, baseline: f32) -> Line {
    line_full(text, size, 72.0, baseline, Style::default())
  }

  fn page(number: u32, lines: Vec<Line>) -> PageContent {
    PageContent {
      number,
      width: 600.0,
      height: 800.0,
      elements: lines.into_iter().map(Element::Line).collect(),
    }
  }

  fn texts(blocks: &[Block]) -> Vec<String> {
    blocks
      .iter()
      .map(|b| match b {
        Block::Heading { level, text } => format!("h{level} {text}"),
        Block::Paragraph(c) => format!("p {}", inline_text(c)),
        Block::ListItem {
          kind,
          depth,
          content,
        } => format!("li{depth} {kind:?} {}", inline_text(content)),
      })
      .collect()
  }

  fn inline_text(content: &[Inline]) -> String {
    content
      .iter()
      .map(|i| match i {
        Inline::Text { text, .. } => text.as_str(),
        Inline::Break => " / ",
      })
      .collect()
  }

  const BODY: &str =
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor.";

  #[test]
  fn detects_headings_by_font_size() {
    let pages: Vec<PageContent> = (1..=3)
      .map(|n| {
        page(
          n,
          vec![
            line(&format!("Capítulo {n}"), 24.0, 50.0),
            line("Seção", 16.0, 90.0),
            line(BODY, 11.0, 120.0),
            line(BODY, 11.0, 134.0),
            line(BODY, 11.0, 148.0),
          ],
        )
      })
      .collect();
    let s = build_blocks(&pages, &[]);
    assert_eq!(s.source, HeadingSource::FontSize);
    let t = texts(&s.blocks);
    assert_eq!(t[0], "h1 Capítulo 1");
    assert_eq!(t[1], "h2 Seção");
    assert!(t[2].starts_with("p Lorem"));
    assert_eq!(split_chapters(s.blocks, 1).len(), 3);
  }

  #[test]
  fn detects_bold_body_headings() {
    let bold = Style {
      bold: true,
      ..Style::default()
    };
    let pages = vec![page(
      1,
      vec![
        line(BODY, 11.0, 100.0),
        line_full("Um subtítulo em negrito", 11.0, 72.0, 130.0, bold),
        line(BODY, 11.0, 150.0),
      ],
    )];
    let t = texts(&build_blocks(&pages, &[]).blocks);
    assert_eq!(t[1], "h1 Um subtítulo em negrito");
  }

  #[test]
  fn chapter_number_line_merges_into_title() {
    let pages: Vec<PageContent> = (1..=2)
      .map(|n| {
        page(
          n,
          vec![
            line(&n.to_string(), 30.0, 60.0),
            line("Um título", 20.0, 100.0),
            line(BODY, 11.0, 130.0),
            line(BODY, 11.0, 144.0),
            line(BODY, 11.0, 158.0),
          ],
        )
      })
      .collect();
    let t = texts(&build_blocks(&pages, &[]).blocks);
    assert_eq!(t[0], "h1 1 Um título");
  }

  #[test]
  fn consecutive_distant_headings_stay_separate() {
    let pages: Vec<PageContent> = (1..=2)
      .map(|n| {
        page(
          n,
          vec![
            line(&format!("Seção A{n}"), 16.0, 60.0),
            line(&format!("Seção B{n}"), 16.0, 200.0),
            line(BODY, 11.0, 230.0),
            line(BODY, 11.0, 244.0),
            line(BODY, 11.0, 258.0),
          ],
        )
      })
      .collect();
    let t = texts(&build_blocks(&pages, &[]).blocks);
    assert_eq!(&t[..2], ["h1 Seção A1", "h1 Seção B1"]);
  }

  #[test]
  fn paragraphs_split_by_first_line_indent() {
    let p = page(
      1,
      vec![
        line_full(BODY, 11.0, 72.0, 100.0, Style::default()),
        line_full(BODY, 11.0, 72.0, 114.0, Style::default()),
        line_full(BODY, 11.0, 86.0, 128.0, Style::default()),
        line_full(BODY, 11.0, 72.0, 142.0, Style::default()),
      ],
    );
    assert_eq!(texts(&build_blocks(&[p], &[]).blocks).len(), 2);
  }

  #[test]
  fn builds_lists_with_continuations_and_nesting() {
    let p = page(
      1,
      vec![
        line(BODY, 11.0, 100.0),
        line_full(
          "1. primeiro item que continua",
          11.0,
          72.0,
          114.0,
          Style::default(),
        ),
        line_full("na linha de baixo", 11.0, 84.0, 128.0, Style::default()),
        line_full("• subitem", 11.0, 90.0, 142.0, Style::default()),
        line_full("2. segundo", 11.0, 72.0, 156.0, Style::default()),
      ],
    );
    let t = texts(&build_blocks(&[p], &[]).blocks);
    assert_eq!(
      t[1],
      "li0 Number(1) primeiro item que continua na linha de baixo"
    );
    assert_eq!(t[2], "li1 Bullet subitem");
    assert_eq!(t[3], "li0 Number(2) segundo");
  }

  #[test]
  fn paragraph_continues_across_pages() {
    let p1 = page(1, vec![line("Uma frase que segue", 11.0, 700.0)]);
    let p2 = page(
      2,
      vec![line("na outra página.", 11.0, 60.0), line(BODY, 11.0, 74.0)],
    );
    let t = texts(&build_blocks(&[p1, p2], &[]).blocks);
    assert_eq!(
      t[0],
      format!("p Uma frase que segue na outra página. {BODY}")
    );
  }

  #[test]
  fn places_outline_entries_and_consumes_title_lines() {
    let pages = vec![
      page(1, vec![line("Prefácio qualquer.", 11.0, 50.0)]),
      page(
        2,
        vec![
          line("5", 30.0, 50.0),
          line("HOW THE KERNEL BOOTS", 20.0, 90.0),
          line("Texto.", 11.0, 130.0),
          line("5.1 Um subtítulo", 14.0, 160.0),
          line("Mais texto.", 11.0, 180.0),
        ],
      ),
    ];
    let outline = vec![
      OutlineEntry {
        level: 1,
        title: "Chapter 5: How the Kernel Boots".into(),
        page: 2,
      },
      OutlineEntry {
        level: 2,
        title: "5.1 Um subtítulo".into(),
        page: 2,
      },
    ];
    let s = build_blocks(&pages, &outline);
    assert_eq!(s.source, HeadingSource::Outline);
    assert_eq!(
      texts(&s.blocks),
      [
        "p Prefácio qualquer.",
        "h1 Chapter 5: How the Kernel Boots",
        "p Texto.",
        "h2 5.1 Um subtítulo",
        "p Mais texto.",
      ]
    );
  }

  #[test]
  fn ignores_outline_that_misses_bigger_chapters() {
    let pages: Vec<PageContent> = (1..=3)
      .map(|n| {
        page(
          n,
          vec![
            line(&format!("Capítulo {n}"), 24.0, 50.0),
            line(&format!("{n}.1 Seção"), 16.0, 90.0),
            line(BODY, 11.0, 120.0),
            line(BODY, 11.0, 134.0),
            line(BODY, 11.0, 148.0),
          ],
        )
      })
      .collect();
    let outline: Vec<OutlineEntry> = (1..=3)
      .map(|n| OutlineEntry {
        level: 1,
        title: format!("{n}.1 Seção"),
        page: n,
      })
      .collect();
    let s = build_blocks(&pages, &outline);
    assert!(s.outline_ignored);
    assert_eq!(split_chapters(s.blocks, 1)[0].title, "Capítulo 1");
  }

  #[test]
  fn keeps_outline_when_big_extra_titles_are_in_one_place() {
    // Ex.: anúncios de outros livros depois do último capítulo.
    let mut pages = vec![page(
      1,
      vec![line("Introdução", 16.0, 50.0), line(BODY, 11.0, 80.0)],
    )];
    for n in 2..=4 {
      pages.push(page(
        n,
        vec![
          line(&format!("Outro livro {n}"), 24.0, 50.0),
          line(BODY, 11.0, 80.0),
        ],
      ));
    }
    let outline = vec![
      OutlineEntry {
        level: 1,
        title: "Introdução".into(),
        page: 1,
      },
      OutlineEntry {
        level: 1,
        title: "Copyright".into(),
        page: 2,
      },
    ];
    assert!(!build_blocks(&pages, &outline).outline_ignored);
  }

  #[test]
  fn auto_split_uses_chapters_under_parts() {
    let mut blocks = vec![];
    for part in 1..=3 {
      blocks.push(Block::Heading {
        level: 1,
        text: format!("Parte {part}"),
      });
      for c in 1..=4 {
        blocks.push(Block::Heading {
          level: 2,
          text: format!("Cap {part}.{c}"),
        });
      }
    }
    assert_eq!(auto_split_level(&blocks), 2);
    let many_chapters: Vec<Block> = (0..15)
      .map(|i| Block::Heading {
        level: 1,
        text: i.to_string(),
      })
      .collect();
    assert_eq!(auto_split_level(&many_chapters), 1);
  }

  #[test]
  fn drops_empty_leaf_chapters_and_keeps_parts() {
    let blocks = vec![
      Block::Heading {
        level: 1,
        text: "Sumário".into(),
      },
      Block::Heading {
        level: 1,
        text: "Parte I".into(),
      },
      Block::Heading {
        level: 2,
        text: "Cap 1".into(),
      },
      Block::Paragraph(vec![Inline::plain("texto")]),
    ];
    let titles: Vec<String> = split_chapters(blocks, 2)
      .into_iter()
      .map(|c| c.title)
      .collect();
    assert_eq!(titles, ["Parte I", "Cap 1"]);
  }

  #[test]
  fn split_depth_never_skips_levels() {
    let blocks = vec![
      Block::Heading {
        level: 2,
        text: "a".into(),
      },
      Block::Paragraph(vec![Inline::plain("x")]),
      Block::Heading {
        level: 1,
        text: "b".into(),
      },
      Block::Paragraph(vec![Inline::plain("x")]),
      Block::Heading {
        level: 2,
        text: "c".into(),
      },
      Block::Paragraph(vec![Inline::plain("x")]),
    ];
    let depths: Vec<u8> = split_chapters(blocks, 2).iter().map(|c| c.depth).collect();
    assert_eq!(depths, vec![1, 1, 2]);
  }
}

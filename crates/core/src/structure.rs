//! Transforma linhas soltas em estrutura: títulos, parágrafos, itens de lista e capítulos.

use std::collections::{HashMap, HashSet};

use crate::extract::{Line, Page};
use crate::outline::OutlineEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
  Heading { level: u8, text: String },
  Paragraph(String),
  ListItem(String),
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

/// Título dado ao conteúdo que aparece antes do primeiro capítulo detectado.
pub const FRONT_MATTER_TITLE: &str = "Início";

/// Quantas linhas do topo e da base de cada página podem ser cabeçalho/rodapé.
const EDGE_LINES: usize = 2;
/// Tamanho a partir do qual uma linha é considerada maior que o corpo do texto.
const HEADING_SIZE_RATIO: f32 = 1.15;
/// Linhas mais longas que isso nunca são títulos.
const MAX_HEADING_CHARS: usize = 120;
/// Espaço vertical (relativo ao entrelinha mediano) que separa parágrafos.
const PARAGRAPH_GAP_RATIO: f32 = 1.25;
/// Níveis de título distintos detectados por tamanho de fonte.
const MAX_FONT_LEVELS: u8 = 3;

/// Remove cabeçalhos e rodapés repetidos e números de página soltos.
pub fn remove_page_furniture(pages: &mut [Page]) {
  let mut counts: HashMap<(String, i32), usize> = HashMap::new();
  for page in pages.iter() {
    let keys: HashSet<(String, i32)> = edge_indices(page.lines.len())
      .map(|i| furniture_key(&page.lines[i]))
      .filter(|(k, _)| !k.is_empty())
      .collect();
    for key in keys {
      *counts.entry(key).or_default() += 1;
    }
  }
  let threshold = (pages.len() * 2).div_ceil(5).max(3);
  let repeated: HashSet<(String, i32)> = counts
    .into_iter()
    .filter(|(_, n)| *n >= threshold)
    .map(|(k, _)| k)
    .collect();

  for page in pages.iter_mut() {
    let edges: HashSet<usize> = edge_indices(page.lines.len()).collect();
    let mut i = 0;
    page.lines.retain(|line| {
      let remove = edges.contains(&i)
        && (is_page_number(&line.text) || repeated.contains(&furniture_key(line)));
      i += 1;
      !remove
    });
  }
}

fn edge_indices(len: usize) -> impl Iterator<Item = usize> {
  let top = 0..EDGE_LINES.min(len);
  let bottom = len.saturating_sub(EDGE_LINES).max(EDGE_LINES.min(len))..len;
  top.chain(bottom)
}

/// Chave de comparação: texto sem números (que mudam de página para página) e posição
/// vertical aproximada, já que cabeçalhos e rodapés ficam sempre no mesmo lugar.
fn furniture_key(line: &Line) -> (String, i32) {
  let text = line
    .text
    .chars()
    .filter(|c| !c.is_ascii_digit())
    .collect::<String>()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase();
  (text, (line.y / 4.0).round() as i32)
}

fn is_page_number(text: &str) -> bool {
  let t = text
    .trim()
    .trim_matches(|c| c == '-' || c == '—' || c == '–')
    .trim();
  let arabic = !t.is_empty() && t.len() <= 4 && t.chars().all(|c| c.is_ascii_digit());
  // Só minúsculas: "I" ou "V" isolados em maiúscula podem ser títulos de capítulo.
  let roman = !t.is_empty() && t.len() <= 6 && t.chars().all(|c| "ivxlc".contains(c));
  arabic || roman
}

enum Item<'a> {
  Heading { level: u8, text: String },
  Line(&'a Line),
}

pub struct Structured {
  pub blocks: Vec<Block>,
  pub source: HeadingSource,
  /// O PDF tinha outline, mas ele deixava de fora títulos principais e foi descartado.
  pub outline_ignored: bool,
}

/// Monta os blocos do livro inteiro, usando o outline quando houver e parecer completo.
pub fn build_blocks(pages: &[Page], outline: &[OutlineEntry]) -> Structured {
  let levels = font_levels(pages);
  let font: Vec<Vec<Item>> = pages.iter().map(|p| font_items(p, &levels)).collect();

  let outline_ignored = !outline.is_empty() && outline_misses_chapters(outline, &font);
  if !outline.is_empty() && !outline_ignored {
    let blocks = assemble(&outline_items(pages, outline));
    return Structured {
      blocks,
      source: HeadingSource::Outline,
      outline_ignored,
    };
  }

  let found = font
    .iter()
    .flatten()
    .any(|i| matches!(i, Item::Heading { .. }));
  let source = if found {
    HeadingSource::FontSize
  } else {
    HeadingSource::None
  };
  Structured {
    blocks: assemble(&font),
    source,
    outline_ignored,
  }
}

/// Outlines incompletos são comuns (ex.: exportadores que só marcam subtítulos).
/// Se a maioria dos títulos de maior fonte não aparece no outline, ele não é confiável.
fn outline_misses_chapters(outline: &[OutlineEntry], font: &[Vec<Item>]) -> bool {
  let titles: Vec<String> = outline.iter().map(|e| normalize(&e.title)).collect();
  let top: Vec<String> = font
    .iter()
    .flatten()
    .filter_map(|i| match i {
      Item::Heading { level: 1, text } => Some(normalize(text)),
      _ => None,
    })
    .filter(|t| !t.is_empty())
    .collect();
  let missing = top
    .iter()
    .filter(|t| {
      !titles
        .iter()
        .any(|o| !o.is_empty() && (o.contains(t.as_str()) || t.contains(o.as_str())))
    })
    .count();
  missing >= 2 && missing * 2 > top.len()
}

/// Mapeia tamanho de fonte (em meio-pontos) → nível de título.
fn font_levels(pages: &[Page]) -> HashMap<u32, u8> {
  let mut chars_by_size: HashMap<u32, usize> = HashMap::new();
  let mut pages_by_size: HashMap<u32, HashSet<u32>> = HashMap::new();
  for page in pages {
    for line in &page.lines {
      let key = size_key(line.font_size);
      *chars_by_size.entry(key).or_default() += line.text.chars().count();
      pages_by_size.entry(key).or_default().insert(page.number);
    }
  }
  let Some((&body, &body_chars)) = chars_by_size.iter().max_by_key(|&(k, n)| (*n, *k)) else {
    return HashMap::new();
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

  candidates
    .into_iter()
    .map(|k| {
      let above = ranked.iter().filter(|&&r| r > k).count() as u8;
      (k, (above + 1).min(MAX_FONT_LEVELS))
    })
    .collect()
}

fn size_key(size: f32) -> u32 {
  (size * 2.0).round() as u32
}

fn font_items<'a>(page: &'a Page, levels: &HashMap<u32, u8>) -> Vec<Item<'a>> {
  let mut items = Vec::with_capacity(page.lines.len());
  for line in &page.lines {
    let level = levels
      .get(&size_key(line.font_size))
      .filter(|_| line.text.chars().count() <= MAX_HEADING_CHARS);
    match (level, items.last_mut()) {
      // Título quebrado em várias linhas: junta numa só.
      (Some(&level), Some(Item::Heading { level: prev, text })) if *prev == level => {
        text.push(' ');
        text.push_str(&line.text);
      }
      (Some(&level), _) => items.push(Item::Heading {
        level,
        text: line.text.clone(),
      }),
      (None, _) => items.push(Item::Line(line)),
    }
  }
  items
}

fn outline_items<'a>(pages: &'a [Page], outline: &[OutlineEntry]) -> Vec<Vec<Item<'a>>> {
  pages
    .iter()
    .map(|page| {
      let entries: Vec<&OutlineEntry> = outline.iter().filter(|e| e.page == page.number).collect();
      outline_page_items(page, &entries)
    })
    .collect()
}

fn outline_page_items<'a>(page: &'a Page, entries: &[&OutlineEntry]) -> Vec<Item<'a>> {
  // Para cada entrada, acha onde o título aparece na página (a partir do anterior).
  let mut placements: Vec<(usize, usize, &OutlineEntry)> = Vec::new(); // (início, fim, entrada)
  let mut cursor = 0;
  for entry in entries {
    let target = normalize(&entry.title);
    let found = (cursor..page.lines.len()).find_map(|start| {
      let mut acc = String::new();
      for end in start..page.lines.len() {
        acc.push_str(&normalize(&page.lines[end].text));
        if acc == target || (acc.starts_with(&target) && end == start) {
          return Some((start, end + 1));
        }
        if acc.is_empty() || !target.starts_with(&acc) {
          return None;
        }
      }
      None
    });
    let (start, end) = found.unwrap_or((cursor, cursor));
    placements.push((start, end, entry));
    cursor = end;
  }

  let mut items = Vec::with_capacity(page.lines.len() + entries.len());
  let mut next = placements.iter().peekable();
  // Linhas que formam o título já viraram o Heading e não se repetem como texto.
  let mut skip_to = 0;
  for (i, line) in page.lines.iter().enumerate() {
    while let Some(&&(start, end, entry)) = next.peek() {
      if start != i {
        break;
      }
      items.push(Item::Heading {
        level: entry.level,
        text: entry.title.clone(),
      });
      skip_to = skip_to.max(end);
      next.next();
    }
    if i >= skip_to {
      items.push(Item::Line(line));
    }
  }
  // Entradas não localizadas no fim da página (ou página sem texto).
  for &(_, _, entry) in next {
    items.push(Item::Heading {
      level: entry.level,
      text: entry.title.clone(),
    });
  }
  items
}

fn normalize(text: &str) -> String {
  deunicode::deunicode(text)
    .chars()
    .filter(|c| c.is_ascii_alphanumeric())
    .map(|c| c.to_ascii_lowercase())
    .collect()
}

/// Junta linhas em parágrafos e itens de lista.
fn assemble(pages: &[Vec<Item>]) -> Vec<Block> {
  let mut blocks = Vec::new();
  let mut current: Option<Block> = None;

  for items in pages {
    let lines: Vec<&Line> = items
      .iter()
      .filter_map(|i| match i {
        Item::Line(l) => Some(*l),
        _ => None,
      })
      .collect();
    let gap = median_gap(&lines);
    let max_len = lines
      .iter()
      .map(|l| l.text.chars().count())
      .max()
      .unwrap_or(0);

    // Parágrafo que termina com pontuação final não continua na próxima página.
    if current
      .as_ref()
      .is_some_and(|b| ends_sentence(block_text(b)))
    {
      blocks.extend(current.take());
    }

    let mut prev: Option<&Line> = None;
    for item in items {
      let line = match item {
        Item::Heading { level, text } => {
          blocks.extend(current.take());
          blocks.push(Block::Heading {
            level: *level,
            text: text.clone(),
          });
          prev = None;
          continue;
        }
        Item::Line(line) => *line,
      };

      let bullet = strip_bullet(&line.text);
      let breaks = match prev {
        None => false,
        Some(p) => {
          let wide_gap = gap.is_some_and(|g| line.y - p.y > g * PARAGRAPH_GAP_RATIO);
          let short_end =
            ends_sentence(&p.text) && (p.text.chars().count() as f32) < max_len as f32 * 0.8;
          wide_gap || short_end
        }
      };

      if current.is_none() || bullet.is_some() || breaks {
        blocks.extend(current.take());
        current = Some(match bullet {
          Some(rest) => Block::ListItem(rest.to_string()),
          None => Block::Paragraph(line.text.clone()),
        });
      } else if let Some(Block::Paragraph(text) | Block::ListItem(text)) = current.as_mut() {
        join_line(text, &line.text);
      }
      prev = Some(line);
    }
  }
  blocks.extend(current);
  blocks
}

fn block_text(block: &Block) -> &str {
  match block {
    Block::Heading { text, .. } | Block::Paragraph(text) | Block::ListItem(text) => text,
  }
}

fn median_gap(lines: &[&Line]) -> Option<f32> {
  let mut gaps: Vec<f32> = lines
    .windows(2)
    .map(|w| w[1].y - w[0].y)
    .filter(|g| *g > 0.0)
    .collect();
  if gaps.is_empty() {
    return None;
  }
  gaps.sort_by(f32::total_cmp);
  Some(gaps[gaps.len() / 2])
}

fn ends_sentence(text: &str) -> bool {
  let t = text.trim_end_matches(['"', '”', '’', ')', '»']);
  t.ends_with(['.', '!', '?', ':'])
}

fn strip_bullet(text: &str) -> Option<&str> {
  let mut chars = text.chars();
  let first = chars.next()?;
  let rest = chars.as_str();
  let is_bullet = matches!(first, '•' | '◦' | '▪' | '‣' | '●' | '○' | '■' | '–')
    || (matches!(first, '-' | '*') && rest.starts_with(' '));
  is_bullet
    .then(|| rest.trim_start())
    .filter(|r| !r.is_empty())
}

/// Junta uma linha ao parágrafo, desfazendo hifenização de fim de linha.
fn join_line(text: &mut String, next: &str) {
  let mut tail = text.chars().rev();
  let hyphenated = tail.next() == Some('-')
    && tail.next().is_some_and(char::is_alphabetic)
    && next.chars().next().is_some_and(char::is_lowercase);
  if hyphenated {
    text.pop();
  } else {
    text.push(' ');
  }
  text.push_str(next);
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

  fn line(text: &str, font_size: f32, y: f32) -> Line {
    Line {
      text: text.to_string(),
      font_size,
      y,
    }
  }

  fn page(number: u32, lines: Vec<Line>) -> Page {
    Page { number, lines }
  }

  #[test]
  fn removes_repeated_headers_and_page_numbers() {
    let bodies = [
      "Texto inicial.",
      "Outro assunto.",
      "Mais conteúdo.",
      "Fim de tudo.",
    ];
    let mut pages: Vec<Page> = (1..=4)
      .map(|n| {
        page(
          n,
          vec![
            line("Meu Livro — Capítulo 1", 9.0, 30.0),
            line(bodies[n as usize - 1], 11.0, 100.0),
            line(&n.to_string(), 9.0, 800.0),
          ],
        )
      })
      .collect();
    remove_page_furniture(&mut pages);
    for (i, p) in pages.iter().enumerate() {
      assert_eq!(p.lines.len(), 1, "página {}", i + 1);
      assert_eq!(p.lines[0].text, bodies[i]);
    }
  }

  #[test]
  fn keeps_body_lines_that_look_like_numbers() {
    let lines: Vec<Line> = (0..6)
      .map(|i| line(&format!("{i}"), 11.0, i as f32 * 14.0))
      .collect();
    let mut pages = vec![page(1, lines)];
    remove_page_furniture(&mut pages);
    // Só as linhas das bordas (2 em cima, 2 embaixo) são candidatas.
    assert_eq!(pages[0].lines.len(), 2);
  }

  #[test]
  fn detects_headings_by_font_size() {
    let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod.";
    let mut pages = Vec::new();
    for n in 1..=3 {
      pages.push(page(
        n,
        vec![
          line(&format!("Capítulo {n}"), 24.0, 50.0),
          line("Seção", 16.0, 90.0),
          line(body, 11.0, 120.0),
          line(body, 11.0, 134.0),
          line(body, 11.0, 148.0),
        ],
      ));
    }
    let Structured { blocks, source, .. } = build_blocks(&pages, &[]);
    assert_eq!(source, HeadingSource::FontSize);
    assert_eq!(
      blocks[0],
      Block::Heading {
        level: 1,
        text: "Capítulo 1".into()
      }
    );
    assert_eq!(
      blocks[1],
      Block::Heading {
        level: 2,
        text: "Seção".into()
      }
    );
    assert!(matches!(blocks[2], Block::Paragraph(_)));

    let chapters = split_chapters(blocks, 1);
    assert_eq!(chapters.len(), 3);
    assert_eq!(chapters[2].title, "Capítulo 3");
  }

  #[test]
  fn single_page_giant_title_does_not_demote_chapters() {
    let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod.";
    let mut pages = vec![page(1, vec![line("O Grande Livro", 40.0, 300.0)])];
    for n in 2..=4 {
      pages.push(page(
        n,
        vec![
          line(&format!("Capítulo {n}"), 20.0, 50.0),
          line(body, 11.0, 100.0),
          line(body, 11.0, 114.0),
          line(body, 11.0, 128.0),
          line(body, 11.0, 142.0),
        ],
      ));
    }
    let levels = font_levels(&pages);
    assert_eq!(levels[&size_key(40.0)], 1);
    assert_eq!(levels[&size_key(20.0)], 1);
  }

  #[test]
  fn merges_multiline_headings() {
    let levels = HashMap::from([(size_key(20.0), 1)]);
    let p = page(
      1,
      vec![
        line("Um título", 20.0, 10.0),
        line("muito longo", 20.0, 34.0),
      ],
    );
    let items = font_items(&p, &levels);
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0], Item::Heading { text, .. } if text == "Um título muito longo"));
  }

  #[test]
  fn builds_paragraphs_with_hyphenation_and_lists() {
    let p = page(
      1,
      vec![
        line("Primeira linha de um parágrafo que conti-", 11.0, 100.0),
        line("nua aqui e termina agora no fim da linha.", 11.0, 114.0),
        line("Outro parágrafo após espaço maior.", 11.0, 140.0),
        line("• item um", 11.0, 154.0),
        line("• item dois", 11.0, 168.0),
      ],
    );
    let items = vec![p.lines.iter().map(Item::Line).collect::<Vec<_>>()];
    let blocks = assemble(&items);
    assert_eq!(
      blocks,
      vec![
        Block::Paragraph(
          "Primeira linha de um parágrafo que continua aqui e termina agora no fim da linha."
            .into()
        ),
        Block::Paragraph("Outro parágrafo após espaço maior.".into()),
        Block::ListItem("item um".into()),
        Block::ListItem("item dois".into()),
      ]
    );
  }

  #[test]
  fn paragraph_continues_across_pages() {
    let p1 = page(1, vec![line("Uma frase que segue", 11.0, 700.0)]);
    let p2 = page(
      2,
      vec![
        line("na outra página.", 11.0, 60.0),
        line(
          "Nova frase, bem mais comprida, que ocupa a largura da página.",
          11.0,
          74.0,
        ),
      ],
    );
    let items: Vec<Vec<Item>> = [&p1, &p2]
      .iter()
      .map(|p| p.lines.iter().map(Item::Line).collect())
      .collect();
    let blocks = assemble(&items);
    assert_eq!(
      blocks[0],
      Block::Paragraph("Uma frase que segue na outra página.".into())
    );
  }

  #[test]
  fn places_outline_entries_on_matching_lines() {
    let pages = vec![
      page(1, vec![line("Prefácio qualquer.", 11.0, 50.0)]),
      page(
        2,
        vec![
          line("1 Introdução", 18.0, 50.0),
          line("Texto.", 11.0, 80.0),
          line("1.1 Um subtítulo", 14.0, 110.0),
          line("Mais texto.", 11.0, 130.0),
        ],
      ),
    ];
    let outline = vec![
      OutlineEntry {
        level: 1,
        title: "1 Introdução".into(),
        page: 2,
      },
      OutlineEntry {
        level: 2,
        title: "1.1 Um subtítulo".into(),
        page: 2,
      },
    ];
    let Structured { blocks, source, .. } = build_blocks(&pages, &outline);
    assert_eq!(source, HeadingSource::Outline);
    assert_eq!(
      blocks,
      vec![
        Block::Paragraph("Prefácio qualquer.".into()),
        Block::Heading {
          level: 1,
          text: "1 Introdução".into()
        },
        Block::Paragraph("Texto.".into()),
        Block::Heading {
          level: 2,
          text: "1.1 Um subtítulo".into()
        },
        Block::Paragraph("Mais texto.".into()),
      ]
    );

    let chapters = split_chapters(blocks, 1);
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, FRONT_MATTER_TITLE);
    assert_eq!(chapters[1].blocks.len(), 3);
  }

  #[test]
  fn ignores_outline_that_misses_chapters() {
    let body = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod.";
    let pages: Vec<Page> = (1..=3)
      .map(|n| {
        page(
          n,
          vec![
            line(&format!("Capítulo {n}"), 24.0, 50.0),
            line(&format!("{n}.1 Seção"), 16.0, 90.0),
            line(body, 11.0, 120.0),
            line(body, 11.0, 134.0),
            line(body, 11.0, 148.0),
          ],
        )
      })
      .collect();
    // Outline só com as seções, como alguns exportadores geram.
    let outline: Vec<OutlineEntry> = (1..=3)
      .map(|n| OutlineEntry {
        level: 1,
        title: format!("{n}.1 Seção"),
        page: n,
      })
      .collect();
    let result = build_blocks(&pages, &outline);
    assert!(result.outline_ignored);
    assert_eq!(result.source, HeadingSource::FontSize);
    assert_eq!(split_chapters(result.blocks, 1)[0].title, "Capítulo 1");
  }

  #[test]
  fn split_depth_never_skips_levels() {
    let blocks = vec![
      Block::Heading {
        level: 2,
        text: "a".into(),
      },
      Block::Heading {
        level: 1,
        text: "b".into(),
      },
      Block::Heading {
        level: 2,
        text: "c".into(),
      },
    ];
    let depths: Vec<u8> = split_chapters(blocks, 2).iter().map(|c| c.depth).collect();
    assert_eq!(depths, vec![1, 1, 2]);
  }
}

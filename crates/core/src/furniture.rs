//! Remove o que não é conteúdo: cabeçalhos, rodapés, números de página e páginas de
//! sumário/índice impressos (o mdBook gera o próprio índice).

use std::collections::{HashMap, HashSet};

use crate::model::{Line, Page};

/// Fração da altura da página, no topo e na base, onde ficam cabeçalhos e rodapés.
const BAND: f32 = 0.085;
/// Quantas páginas precisam repetir uma linha para ela ser considerada cabeçalho/rodapé.
const MIN_REPEATS: usize = 3;

pub fn remove_headers_and_footers(pages: &mut [Page]) {
  let mut counts: HashMap<(String, i32), usize> = HashMap::new();
  for page in pages.iter() {
    let keys: HashSet<(String, i32)> = page
      .lines
      .iter()
      .filter(|line| in_band(line, page.height))
      .map(key)
      .filter(|(text, _)| !text.is_empty())
      .collect();
    for key in keys {
      *counts.entry(key).or_default() += 1;
    }
  }
  let threshold = MIN_REPEATS.min(pages.len()).max(2);
  let repeated: HashSet<(String, i32)> = counts
    .into_iter()
    .filter(|(_, n)| *n >= threshold)
    .map(|(k, _)| k)
    .collect();

  for page in pages.iter_mut() {
    let height = page.height;
    page.lines.retain(|line| {
      !(in_band(line, height) && (is_page_number(&line.text()) || repeated.contains(&key(line))))
    });
  }
}

fn in_band(line: &Line, height: f32) -> bool {
  line.bbox.y1 <= height * BAND || line.bbox.y0 >= height * (1.0 - BAND)
}

/// Texto sem números (que mudam de página para página) e posição vertical aproximada.
fn key(line: &Line) -> (String, i32) {
  let text = line
    .text()
    .chars()
    .filter(|c| !c.is_ascii_digit())
    .collect::<String>()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_lowercase();
  (text, (line.bbox.y0 / 6.0).round() as i32)
}

fn is_page_number(text: &str) -> bool {
  let t = text
    .trim()
    .trim_matches(|c: char| c == '-' || c == '—' || c == '–' || c == '|')
    .trim();
  let arabic = !t.is_empty() && t.len() <= 4 && t.chars().all(|c| c.is_ascii_digit());
  // Só minúsculas: "I" ou "V" isolados em maiúscula podem ser títulos de capítulo.
  let roman = !t.is_empty() && t.len() <= 6 && t.chars().all(|c| "ivxlc".contains(c));
  arabic || roman
}

/// Esvazia páginas de sumário e de índice remissivo: linhas terminadas em número de página
/// ou com pontilhado. O sumário só é procurado no início e o índice no fim do livro, para não
/// confundir com tabelas numéricas no meio do texto.
pub fn remove_reference_pages(pages: &mut [Page]) {
  let total = pages.len().max(1) as f32;
  for (i, page) in pages.iter_mut().enumerate() {
    let position = i as f32 / total;
    if is_reference_page(&page.lines, position) {
      page.lines.clear();
    }
  }
}

fn is_reference_page(lines: &[Line], position: f32) -> bool {
  if lines.len() < 5 {
    return false;
  }
  let texts: Vec<String> = lines.iter().map(Line::text).collect();
  let leaders = texts.iter().filter(|t| has_leader(t)).count();
  let refs = texts
    .iter()
    .filter(|t| has_leader(t) || ends_with_page_ref(t))
    .count();
  let ratio = refs as f32 / texts.len() as f32;
  (leaders >= 3 && ratio >= 0.4)
    || (refs >= 6 && ratio >= 0.5 && !(0.12..=0.85).contains(&position))
}

fn has_leader(text: &str) -> bool {
  text.contains("....") || text.contains(". . . .") || text.contains("····") || text.contains("……")
}

/// "Processos, 45, 112" ou "2.1 Shell 36".
fn ends_with_page_ref(text: &str) -> bool {
  let trimmed = text.trim_end();
  let digits = trimmed
    .chars()
    .rev()
    .take_while(char::is_ascii_digit)
    .count();
  if digits == 0 || digits > 4 || digits == trimmed.chars().count() {
    return false;
  }
  let before = trimmed[..trimmed.len() - digits].chars().next_back();
  matches!(before, Some(' ' | ',' | '.' | '–' | '-'))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{Rect, Span, Style};

  pub(crate) fn line_at(text: &str, y: f32) -> Line {
    let bbox = Rect::new(72.0, y, 72.0 + text.len() as f32 * 5.0, y + 10.0);
    Line {
      spans: vec![Span {
        text: text.to_string(),
        bbox,
        size: 10.0,
        baseline: y + 8.0,
        style: Style::default(),
        space_before: false,
      }],
      bbox,
      size: 10.0,
      baseline: y + 8.0,
    }
  }

  fn page(number: u32, lines: Vec<Line>) -> Page {
    Page {
      number,
      width: 600.0,
      height: 800.0,
      lines,
      graphics: Vec::new(),
    }
  }

  #[test]
  fn removes_running_heads_and_page_numbers_by_position() {
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
            line_at(&format!("{n} Capítulo 1 — Meu Livro"), 30.0),
            line_at(bodies[n as usize - 1], 300.0),
            line_at(&n.to_string(), 770.0),
          ],
        )
      })
      .collect();
    remove_headers_and_footers(&mut pages);
    for (i, p) in pages.iter().enumerate() {
      let texts: Vec<String> = p.lines.iter().map(Line::text).collect();
      assert_eq!(texts, [bodies[i]]);
    }
  }

  #[test]
  fn keeps_repeated_text_in_the_body() {
    let mut pages: Vec<Page> = (1..=4)
      .map(|n| page(n, vec![line_at("Repetido no meio da página.", 300.0)]))
      .collect();
    remove_headers_and_footers(&mut pages);
    assert!(pages.iter().all(|p| p.lines.len() == 1));
  }

  #[test]
  fn detects_toc_and_index_pages() {
    let toc: Vec<Line> = (0..8)
      .map(|i| {
        line_at(
          &format!("{i}.1 Seção qualquer . . . . . . {}", i * 10 + 3),
          100.0 + i as f32 * 14.0,
        )
      })
      .collect();
    assert!(is_reference_page(&toc, 0.02));
    let index: Vec<Line> = (0..10)
      .map(|i| {
        line_at(
          &format!("termo {i}, {}, {}", i + 3, i * 7 + 20),
          100.0 + i as f32 * 14.0,
        )
      })
      .collect();
    assert!(is_reference_page(&index, 0.97));
    // A mesma página no meio do livro é conteúdo (ex.: tabela numérica).
    assert!(!is_reference_page(&index, 0.5));
  }
}

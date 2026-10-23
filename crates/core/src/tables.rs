//! Tabelas delimitadas por filetes: a região vem dos filetes horizontais; as colunas, de filetes
//! verticais, das quebras nos filetes horizontais ou de faixas verticais sem texto; as linhas,
//! dos filetes internos ou das linhas de base do texto.

use crate::model::{GraphicKind, Line, Page, Rect, Script, Span, Style};
use crate::structure::Inline;

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
  pub bbox: Rect,
  /// Primeira linha é o cabeçalho; todas têm o mesmo número de células.
  pub rows: Vec<Vec<Vec<Inline>>>,
}

/// Espessura máxima de um filete.
const RULE: f32 = 2.5;
/// Tolerância para considerar dois filetes na mesma altura ou extensão.
const ALIGN: f32 = 4.0;

#[derive(Debug, Clone, Copy)]
struct HRule {
  y: f32,
  x0: f32,
  x1: f32,
}

/// Extrai as tabelas da página, removendo dela as linhas de texto que viraram células.
pub fn extract_tables(page: &mut Page, body: f32) -> Vec<Table> {
  let (hrules, breaks, vrules) = rules(page);
  let mut tables = Vec::new();
  for group in group_rules(&hrules) {
    if looks_like_drawing(page, &group) {
      continue;
    }
    if let Some(table) = build(page, &group, &breaks, &vrules, body) {
      tables.push(table);
    }
  }
  page.lines.retain(|l| {
    !tables
      .iter()
      .any(|t: &Table| t.bbox.contains_point(l.bbox.center()))
  });
  tables
}

/// Filetes horizontais (segmentos colineares unidos), pontos onde eles se partem e filetes
/// verticais. Bordas de retângulos (células, faixas de fundo) contam como filetes.
fn rules(page: &Page) -> (Vec<HRule>, Vec<(f32, f32)>, Vec<Rect>) {
  let mut segments: Vec<HRule> = Vec::new();
  let mut vrules = Vec::new();
  for g in &page.graphics {
    if !matches!(g.kind, GraphicKind::Path { .. }) {
      continue;
    }
    let b = g.bbox;
    if b.height() <= RULE && b.width() >= 8.0 {
      segments.push(HRule {
        y: (b.y0 + b.y1) / 2.0,
        x0: b.x0,
        x1: b.x1,
      });
    } else if b.width() <= RULE && b.height() >= 6.0 {
      vrules.push(b);
    } else if b.width() >= 8.0 && b.height() > RULE && b.height() <= 80.0 {
      segments.push(HRule {
        y: b.y0,
        x0: b.x0,
        x1: b.x1,
      });
      segments.push(HRule {
        y: b.y1,
        x0: b.x0,
        x1: b.x1,
      });
    }
  }
  segments.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x0.total_cmp(&b.x0)));

  let mut merged: Vec<HRule> = Vec::new();
  let mut breaks = Vec::new(); // (y, x) onde um filete se parte em dois
  for seg in segments {
    match merged.last_mut() {
      Some(last) if (last.y - seg.y).abs() <= 1.5 && seg.x0 <= last.x1 + 3.0 => {
        if seg.x0 > last.x0 + 1.0 && seg.x0 >= last.x1 - 3.0 {
          breaks.push((last.y, (seg.x0 + last.x1) / 2.0));
        }
        last.x1 = last.x1.max(seg.x1);
      }
      _ => merged.push(seg),
    }
  }
  (merged, breaks, vrules)
}

/// Filetes empilhados com a mesma extensão horizontal formam uma tabela candidata (outros
/// filetes no meio, de extensão diferente, não quebram o grupo).
fn group_rules(rules: &[HRule]) -> Vec<Vec<HRule>> {
  let mut groups: Vec<Vec<HRule>> = Vec::new();
  for &rule in rules {
    let open = groups.iter_mut().rev().find(|g| {
      let (first, last) = (g[0], g[g.len() - 1]);
      (first.x0 - rule.x0).abs() <= ALIGN * 3.0
        && (first.x1 - rule.x1).abs() <= ALIGN * 3.0
        && rule.y - last.y > 1.5
        && rule.y - last.y < 120.0
    });
    match open {
      Some(group) => group.push(rule),
      None => groups.push(vec![rule]),
    }
  }
  groups.retain(|g| g.len() >= 2);
  groups
}

/// Diagrama de caixas (setas, ícones, imagens) não é tabela, mesmo tendo retângulos alinhados.
fn looks_like_drawing(page: &Page, group: &[HRule]) -> bool {
  let x0 = group.iter().map(|r| r.x0).fold(f32::MAX, f32::min);
  let x1 = group.iter().map(|r| r.x1).fold(f32::MIN, f32::max);
  let region = Rect::new(
    x0 - 2.0,
    group[0].y - 2.0,
    x1 + 2.0,
    group[group.len() - 1].y + 2.0,
  );
  let mut marks = 0;
  for g in page
    .graphics
    .iter()
    .filter(|g| region.contains_point(g.bbox.center()))
  {
    match g.kind {
      GraphicKind::Image { .. } => return true,
      GraphicKind::Path { .. } => {
        let small = g.bbox.width() < 8.0
          && g.bbox.height() < 8.0
          && g.bbox.width() > RULE
          && g.bbox.height() > RULE;
        let diagonal = g.bbox.width() > RULE
          && g.bbox.height() > RULE
          && g.bbox.height() < 80.0
          && g.bbox.width() < 80.0;
        if small || (diagonal && g.bbox.width() < 30.0 && g.bbox.height() > 8.0) {
          marks += 1;
        }
      }
    }
  }
  marks >= 3
}

fn build(
  page: &Page,
  group: &[HRule],
  breaks: &[(f32, f32)],
  vrules: &[Rect],
  body: f32,
) -> Option<Table> {
  let x0 = group.iter().map(|r| r.x0).fold(f32::MAX, f32::min);
  let x1 = group.iter().map(|r| r.x1).fold(f32::MIN, f32::max);
  let (top, bottom) = (group[0].y, group[group.len() - 1].y);
  let bbox = Rect::new(x0 - 2.0, top - 1.0, x1 + 2.0, bottom + 1.0);
  if bbox.height() < 12.0 {
    return None;
  }
  let lines: Vec<&Line> = page
    .lines
    .iter()
    .filter(|l| bbox.contains_point(l.bbox.center()))
    .collect();
  if lines.len() < 2 {
    return None;
  }
  // Texto que transborda a região não é célula: é prosa entre dois filetes.
  if lines
    .iter()
    .any(|l| l.bbox.x0 < x0 - ALIGN || l.bbox.x1 > x1 + ALIGN)
  {
    return None;
  }
  // Listagem de código emoldurada por filetes.
  let chars: usize = lines.iter().map(|l| l.char_count()).sum();
  let mono: usize = lines
    .iter()
    .flat_map(|l| &l.spans)
    .filter(|s| s.style.mono)
    .map(|s| s.text.chars().count())
    .sum();
  if mono * 2 >= chars {
    return None;
  }

  let spans: Vec<&Span> = lines
    .iter()
    .flat_map(|l| l.spans.iter())
    .filter(|s| !s.text.trim().is_empty())
    .collect();
  let boundaries = column_boundaries(&bbox, &spans, breaks, vrules, body);
  if boundaries.is_empty() {
    return None;
  }
  let columns = boundaries.len() + 1;
  let column_of = |span: &Span| {
    boundaries
      .iter()
      .filter(|&&b| span.bbox.center().0 > b)
      .count()
  };

  // Linhas da tabela: faixas entre filetes internos (grade completa) ou por linha de base.
  let internal: Vec<f32> = group[1..group.len() - 1].iter().map(|r| r.y).collect();
  let mut row_lines: Vec<Vec<&Line>> = Vec::new();
  let grid = internal.len() + 1 >= 3;
  if grid {
    let mut edges = vec![top];
    edges.extend(&internal);
    edges.push(bottom);
    for pair in edges.windows(2) {
      let band: Vec<&Line> = lines
        .iter()
        .copied()
        .filter(|l| (pair[0]..pair[1]).contains(&l.bbox.center().1))
        .collect();
      if !band.is_empty() {
        row_lines.push(band);
      }
    }
  } else {
    // Cabeçalho: o que fica acima do primeiro filete interno, numa linha só.
    let header_end = internal.first().copied().unwrap_or(f32::MIN);
    let mut sorted = lines.clone();
    sorted.sort_by(|a, b| a.baseline.total_cmp(&b.baseline));
    for line in sorted {
      let in_header = line.bbox.center().1 < header_end;
      match row_lines.last_mut() {
        Some(row) if in_header && row[0].bbox.center().1 < header_end => row.push(line),
        Some(row) if (row[0].baseline - line.baseline).abs() <= line.size * 0.5 => row.push(line),
        _ => row_lines.push(vec![line]),
      }
    }
  }

  let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
  for mut band in row_lines {
    band.sort_by(|a, b| a.baseline.total_cmp(&b.baseline));
    let mut cells: Vec<Vec<Inline>> = vec![Vec::new(); columns];
    for line in &band {
      for span in line.spans.iter().filter(|s| !s.text.trim().is_empty()) {
        push_span(&mut cells[column_of(span)], line, span);
      }
    }
    let row = cells;
    // Linha cuja primeira célula está vazia continua a de cima (texto quebrado na célula).
    let continuation = !grid && rows.len() > 1 && row[0].is_empty();
    match rows.last_mut() {
      Some(prev) if continuation => {
        for (cell, more) in prev.iter_mut().zip(row) {
          if !more.is_empty() {
            if !cell.is_empty() {
              cell.push(Inline::plain(" "));
            }
            cell.extend(more);
          }
        }
      }
      _ => rows.push(row),
    }
  }

  // Colunas vazias em todas as linhas são fronteiras espúrias.
  let used: Vec<bool> = (0..columns)
    .map(|c| rows.iter().any(|r| !r[c].is_empty()))
    .collect();
  for row in &mut rows {
    let mut i = 0;
    row.retain(|_| {
      i += 1;
      used[i - 1]
    });
  }
  let columns = rows.first().map_or(0, Vec::len);

  let filled_rows = rows
    .iter()
    .skip(1)
    .filter(|r| r.iter().filter(|c| !c.is_empty()).count() >= 2)
    .count();
  if columns < 2 || rows.len() < 2 || filled_rows * 2 < rows.len() - 1 {
    return None;
  }
  if is_prose(&rows) || is_drawn_array(&rows) {
    return None;
  }
  Some(Table { bbox, rows })
}

fn cell_text(cell: &[Inline]) -> String {
  cell
    .iter()
    .map(|i| match i {
      Inline::Text { text, .. } | Inline::Sup(text) | Inline::Sub(text) => text.as_str(),
      _ => "",
    })
    .collect()
}

/// Duas colunas de texto corrido entre filetes: palavras hifenizadas no fim das "células" ou
/// todas as células longas.
fn is_prose(rows: &[Vec<Vec<Inline>>]) -> bool {
  let texts: Vec<Vec<String>> = rows
    .iter()
    .map(|r| r.iter().map(|c| cell_text(c)).collect())
    .collect();
  let hyphenated = texts
    .iter()
    .filter(|r| r.iter().any(|t| t.ends_with('\u{ad}') || t.ends_with('-')))
    .count();
  let all_long = texts
    .iter()
    .filter(|r| r.iter().all(|t| t.chars().count() >= 25))
    .count();
  hyphenated >= 2 || all_long * 10 >= texts.len() * 7
}

/// Vetor ou matriz desenhada (caixinhas com um número cada), comum em livros de algoritmos e em
/// figuras: sem cabeçalho e só com células curtíssimas, fica como figura.
fn is_drawn_array(rows: &[Vec<Vec<Inline>>]) -> bool {
  rows
    .iter()
    .flatten()
    .all(|c| cell_text(c).chars().count() <= 3)
}

/// Fronteiras entre colunas: filetes verticais, quebras alinhadas nos filetes horizontais ou,
/// sem eles, faixas verticais que nenhum trecho de texto ocupa.
fn column_boundaries(
  bbox: &Rect,
  spans: &[&Span],
  breaks: &[(f32, f32)],
  vrules: &[Rect],
  body: f32,
) -> Vec<f32> {
  let mut xs: Vec<f32> = vrules
    .iter()
    .filter(|v| {
      let x = v.center().0;
      x > bbox.x0 + ALIGN
        && x < bbox.x1 - ALIGN
        && v.height() >= bbox.height() * 0.5
        && v.y0 >= bbox.y0 - 2.0
        && v.y1 <= bbox.y1 + 2.0
    })
    .map(|v| v.center().0)
    .collect();
  if xs.is_empty() {
    xs = breaks
      .iter()
      .filter(|(y, x)| {
        *y >= bbox.y0 && *y <= bbox.y1 && *x > bbox.x0 + ALIGN && *x < bbox.x1 - ALIGN
      })
      .map(|(_, x)| *x)
      .collect();
  }
  if xs.is_empty() {
    // Vãos sem texto, de pelo menos meio corpo de largura.
    let mut intervals: Vec<(f32, f32)> = spans.iter().map(|s| (s.bbox.x0, s.bbox.x1)).collect();
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end = f32::MIN;
    for (a, b) in intervals {
      if end > f32::MIN && a - end >= body.max(4.0) * 0.5 {
        xs.push((a + end) / 2.0);
      }
      end = end.max(b);
    }
  }
  xs.sort_by(f32::total_cmp);
  xs.dedup_by(|a, b| (*a - *b).abs() < ALIGN);
  xs
}

/// Acrescenta o trecho à célula, na ordem de leitura; expoentes e índices viram <sup>/<sub>.
fn push_span(cell: &mut Vec<Inline>, line: &Line, span: &Span) {
  let text = span.text.trim().to_string();
  match line.script(span) {
    Script::Super => return cell.push(Inline::Sup(text)),
    Script::Sub => return cell.push(Inline::Sub(text)),
    Script::Normal => {}
  }
  // Negrito no cabeçalho é o padrão das tabelas; não precisa de marcação.
  let style = Style {
    bold: false,
    ..span.style
  };
  match cell.last_mut() {
    Some(Inline::Text {
      text: prev,
      style: s,
    }) if *s == style => {
      prev.push(' ');
      prev.push_str(&text);
    }
    Some(_) => {
      cell.push(Inline::plain(" "));
      cell.push(Inline::Text { text, style });
    }
    None => cell.push(Inline::Text { text, style }),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::Graphic;

  fn rule(x0: f32, x1: f32, y: f32) -> Graphic {
    Graphic {
      kind: GraphicKind::Path { filled: false },
      bbox: Rect::new(x0, y, x1, y + 1.0),
    }
  }

  fn line(cells: &[(&str, f32)], y: f32, style: Style) -> Line {
    let spans: Vec<Span> = cells
      .iter()
      .map(|&(t, x)| {
        let bbox = Rect::new(x, y, x + t.len() as f32 * 5.0, y + 10.0);
        Span {
          text: t.into(),
          bbox,
          size: 10.0,
          baseline: y + 8.0,
          style,
          space_before: true,
        }
      })
      .collect();
    let bbox = spans
      .iter()
      .skip(1)
      .fold(spans[0].bbox, |a, s| a.union(&s.bbox));
    Line {
      spans,
      bbox,
      size: 10.0,
      baseline: y + 8.0,
    }
  }

  fn texts(table: &Table) -> Vec<Vec<String>> {
    table
      .rows
      .iter()
      .map(|r| {
        r.iter()
          .map(|c| {
            c.iter()
              .map(|i| match i {
                Inline::Text { text, .. } => text.clone(),
                _ => String::new(),
              })
              .collect()
          })
          .collect()
      })
      .collect()
  }

  fn page(lines: Vec<Line>, graphics: Vec<Graphic>) -> Page {
    Page {
      number: 1,
      width: 600.0,
      height: 800.0,
      lines,
      graphics,
    }
  }

  #[test]
  fn booktabs_table_with_columns_from_text_gaps() {
    let p = &mut page(
      vec![
        line(
          &[("Operador", 100.0), ("Descrição", 250.0)],
          102.0,
          Style::default(),
        ),
        line(
          &[("+", 100.0), ("Soma dois números", 250.0)],
          122.0,
          Style::default(),
        ),
        line(&[("-", 100.0), ("Subtrai", 250.0)], 136.0, Style::default()),
        line(&[("continua a descrição", 250.0)], 148.0, Style::default()),
        line(
          &[("Texto depois da tabela, bem mais largo que ela.", 50.0)],
          200.0,
          Style::default(),
        ),
      ],
      vec![
        rule(95.0, 420.0, 98.0),
        rule(95.0, 420.0, 116.0),
        rule(95.0, 420.0, 162.0),
      ],
    );
    let tables = extract_tables(p, 10.0);
    assert_eq!(tables.len(), 1);
    assert_eq!(
      texts(&tables[0]),
      [
        vec!["Operador", "Descrição"],
        vec!["+", "Soma dois números"],
        vec!["-", "Subtrai continua a descrição"],
      ]
    );
    assert_eq!(p.lines.len(), 1, "só a prosa fica na página");
  }

  #[test]
  fn split_rules_mark_column_edges() {
    let mut graphics = Vec::new();
    for y in [98.0, 116.0, 150.0] {
      graphics.push(rule(95.0, 190.0, y));
      graphics.push(rule(188.0, 300.0, y));
    }
    let p = &mut page(
      vec![
        line(&[("Op Testa", 100.0)], 102.0, Style::default()),
        line(
          &[("-f", 100.0), ("arquivo", 194.0)],
          120.0,
          Style::default(),
        ),
        line(
          &[("-d", 100.0), ("diretório", 194.0)],
          134.0,
          Style::default(),
        ),
      ],
      graphics,
    );
    let tables = extract_tables(p, 10.0);
    assert_eq!(texts(&tables[0])[1], ["-f", "arquivo"]);
  }

  #[test]
  fn two_columns_of_prose_between_rules_are_not_a_table() {
    let rows: Vec<Line> = (0..4)
      .map(|i| {
        line(
          &[
            ("palavras de um texto corrido que segue\u{ad}", 100.0),
            ("e a outra coluna também continua\u{ad}", 330.0),
          ],
          102.0 + i as f32 * 14.0,
          Style::default(),
        )
      })
      .collect();
    let p = &mut page(
      rows,
      vec![rule(95.0, 560.0, 98.0), rule(95.0, 560.0, 160.0)],
    );
    assert!(extract_tables(p, 10.0).is_empty());
  }

  #[test]
  fn boxes_with_arrows_are_a_drawing() {
    let mut graphics = vec![
      rule(95.0, 420.0, 98.0),
      rule(95.0, 420.0, 116.0),
      rule(95.0, 420.0, 150.0),
    ];
    for x in [150.0, 200.0, 250.0] {
      graphics.push(Graphic {
        kind: GraphicKind::Path { filled: true },
        bbox: Rect::new(x, 120.0, x + 5.0, 125.0),
      });
    }
    let p = &mut page(
      vec![
        line(
          &[("Origem", 100.0), ("Destino", 250.0)],
          102.0,
          Style::default(),
        ),
        line(&[("A", 100.0), ("B", 250.0)], 130.0, Style::default()),
      ],
      graphics,
    );
    assert!(extract_tables(p, 10.0).is_empty());
  }

  #[test]
  fn framed_code_and_framed_prose_are_not_tables() {
    let mono = Style {
      mono: true,
      ..Style::default()
    };
    let code = &mut page(
      vec![
        line(&[("for x in *; do", 100.0)], 102.0, mono),
        line(&[("echo $x", 120.0), ("# comentário", 260.0)], 116.0, mono),
      ],
      vec![rule(95.0, 420.0, 98.0), rule(95.0, 420.0, 132.0)],
    );
    assert!(extract_tables(code, 10.0).is_empty());
    let prose = &mut page(
      vec![
        line(
          &[("Um parágrafo qualquer entre filetes.", 100.0)],
          102.0,
          Style::default(),
        ),
        line(&[("Que continua aqui.", 100.0)], 116.0, Style::default()),
      ],
      vec![rule(95.0, 420.0, 98.0), rule(95.0, 420.0, 132.0)],
    );
    assert!(extract_tables(prose, 10.0).is_empty());
  }
}

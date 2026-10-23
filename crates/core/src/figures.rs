//! Figuras: imagens raster e desenhos vetoriais. Traços e imagens próximos são agrupados; o
//! grupo vira um arquivo de imagem (a imagem original, se for uma só, ou um recorte renderizado
//! da página) e o texto dentro dele (rótulos do desenho) sai do corpo do texto.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use pdfium_render::prelude::*;

use crate::extract::ImageAsset;
use crate::model::{GraphicKind, Line, Page, Rect};

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
  pub bbox: Rect,
  /// Nome do arquivo no diretório de recursos.
  pub file: String,
  /// Texto alternativo: a legenda, quando encontrada.
  pub alt: String,
}

/// Escala da renderização de figuras vetoriais (2 × 72 dpi).
const RENDER_SCALE: f32 = 2.0;
/// Distância máxima entre traços do mesmo desenho.
const CLUSTER_PAD: f32 = 4.0;
/// Distância máxima entre partes da mesma figura (subfiguras, rótulos).
const MERGE_GAP: f32 = 18.0;
/// Rótulos curtos (até tantos caracteres) encostados no desenho fazem parte da figura.
const LABEL_CHARS: usize = 30;
/// Texto deste tamanho (relativo ao corpo) é título, nunca rótulo de figura.
const HEADING_RATIO: f32 = 1.3;
/// Imagens que se repetem em tantas páginas são logotipos, fundos ou marcas d'água.
const DECORATIVE_PAGES: usize = 3;

/// Imagens repetidas em várias páginas (decorativas).
pub fn decorative_assets(assets: &[ImageAsset]) -> HashSet<usize> {
  assets
    .iter()
    .enumerate()
    .filter(|(_, a)| a.pages.len() >= DECORATIVE_PAGES)
    .map(|(i, _)| i)
    .collect()
}

pub struct Candidate {
  bbox: Rect,
  /// Imagem original a usar, quando a figura é uma única imagem sem desenho por cima.
  raster: Option<usize>,
}

/// Posição aproximada de um retângulo, para reconhecer o mesmo elemento em páginas diferentes.
fn position_key(r: &Rect) -> [i32; 4] {
  [r.x0, r.y0, r.x1, r.y1].map(|v| (v / 3.0).round() as i32)
}

/// Traços que se repetem na mesma posição em várias páginas: faixas e molduras de template
/// de slide, ornamentos e filetes de página.
pub fn repeated_paths(pages: &[Page]) -> HashSet<[i32; 4]> {
  let mut counts: HashMap<[i32; 4], usize> = HashMap::new();
  for page in pages {
    let keys: HashSet<[i32; 4]> = page
      .graphics
      .iter()
      .filter(|g| matches!(g.kind, GraphicKind::Path { .. }))
      .map(|g| position_key(&g.bbox))
      .collect();
    for k in keys {
      *counts.entry(k).or_default() += 1;
    }
  }
  counts
    .into_iter()
    .filter(|(_, n)| *n >= DECORATIVE_PAGES)
    .map(|(k, _)| k)
    .collect()
}

/// Remove figuras que se repetem na mesma posição em várias páginas.
pub fn drop_repeated(per_page: &mut [Vec<Candidate>]) {
  let key = position_key;
  let mut counts: HashMap<[i32; 4], usize> = HashMap::new();
  for candidates in per_page.iter() {
    let keys: HashSet<[i32; 4]> = candidates.iter().map(|c| key(&c.bbox)).collect();
    for k in keys {
      *counts.entry(k).or_default() += 1;
    }
  }
  for candidates in per_page.iter_mut() {
    candidates.retain(|c| counts[&key(&c.bbox)] < DECORATIVE_PAGES);
  }
}

/// Grava os arquivos das figuras detectadas em `assets_dir` e remove da página as linhas que
/// são rótulos internos (já estão na imagem).
pub fn extract_figures(
  document: &PdfDocument,
  page: &mut Page,
  candidates: Vec<Candidate>,
  assets: &[ImageAsset],
  body: f32,
  assets_dir: &Path,
) -> Vec<Figure> {
  if candidates.is_empty() {
    return Vec::new();
  }

  let mut rendered: Option<image::DynamicImage> = None;
  let mut figures = Vec::new();
  for (k, candidate) in candidates.into_iter().enumerate() {
    let file = match candidate.raster {
      Some(asset) => assets[asset].file.clone(),
      None => {
        if rendered.is_none() {
          rendered = render_page(document, page.number);
        }
        let Some(full) = &rendered else { continue };
        let file = format!("fig-{:04}-{}.png", page.number, k + 1);
        let crop = crop_rect(&candidate.bbox, full.width(), full.height());
        let (x, y, w, h) = crop;
        if w < 8
          || h < 8
          || full
            .crop_imm(x, y, w, h)
            .save(assets_dir.join(&file))
            .is_err()
        {
          continue;
        }
        file
      }
    };
    figures.push(Figure {
      bbox: candidate.bbox,
      file,
      alt: caption(&page.lines, &candidate.bbox).unwrap_or_default(),
    });
  }

  // Rótulos do desenho já estão na imagem.
  page
    .lines
    .retain(|line| is_heading(line, body) || !figures.iter().any(|f| inside(line, &f.bbox)));
  figures
}

fn is_heading(line: &Line, body: f32) -> bool {
  body > 0.0 && line.size >= body * HEADING_RATIO
}

/// Elementos decorativos que não fazem parte de figuras.
#[derive(Default)]
pub struct Ignore {
  pub images: HashSet<usize>,
  pub paths: HashSet<[i32; 4]>,
}

/// Candidatas a figura da página (sem gravar nada). `taken` são regiões já usadas (tabelas).
pub fn detect(page: &Page, ignore: &Ignore, taken: &[Rect], body: f32) -> Vec<Candidate> {
  let page_rect = Rect::new(0.0, 0.0, page.width, page.height);
  let page_area = page_rect.area();
  let few_lines = page.lines.len() < 3;

  // (retângulo, imagem) de cada elemento gráfico relevante.
  let mut items: Vec<(Rect, Option<usize>)> = Vec::new();
  for graphic in &page.graphics {
    let bbox = graphic.bbox;
    if taken.iter().any(|r| r.contains_point(bbox.center())) {
      continue;
    }
    match graphic.kind {
      GraphicKind::Image { asset } => {
        let background = bbox.area() >= page_area * 0.85 && !few_lines;
        if ignore.images.contains(&asset)
          || background
          || bbox.width() < 16.0
          || bbox.height() < 16.0
        {
          continue;
        }
        items.push((bbox, Some(asset)));
      }
      GraphicKind::Path { .. } => {
        let degenerate = bbox.width() < 0.5 && bbox.height() < 0.5;
        // Filetes horizontais compridos separam texto (cabeçalho, rodapé, notas).
        let rule = (bbox.height() < 1.5 && bbox.width() > page.width * 0.25)
          || (bbox.width() < 1.5 && bbox.height() > page.height * 0.4);
        let background = bbox.area() >= page_area * 0.85;
        let repeated = ignore.paths.contains(&position_key(&bbox));
        if degenerate || rule || background || repeated {
          continue;
        }
        items.push((bbox, None));
      }
    }
  }
  if items.is_empty() {
    return Vec::new();
  }

  // Agrupa elementos que se tocam (com folga), por união e busca.
  let mut parent: Vec<usize> = (0..items.len()).collect();
  fn find(parent: &mut [usize], i: usize) -> usize {
    let mut root = i;
    while parent[root] != root {
      root = parent[root];
    }
    let mut node = i;
    while parent[node] != root {
      let next = parent[node];
      parent[node] = root;
      node = next;
    }
    root
  }
  let mut order: Vec<usize> = (0..items.len()).collect();
  order.sort_by(|&a, &b| items[a].0.x0.total_cmp(&items[b].0.x0));
  for (pos, &i) in order.iter().enumerate() {
    let a = items[i].0.expand(CLUSTER_PAD);
    for &j in &order[pos + 1..] {
      if items[j].0.x0 > a.x1 {
        break;
      }
      if a.intersects(&items[j].0) {
        let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
        if ri != rj {
          parent[ri] = rj;
        }
      }
    }
  }

  let mut clusters: Vec<(Rect, usize, Vec<usize>)> = Vec::new(); // (bbox, traços, imagens)
  let mut index_of = HashMap::new();
  for (i, &(bbox, asset)) in items.iter().enumerate() {
    let root = find(&mut parent, i);
    let slot = *index_of.entry(root).or_insert_with(|| {
      clusters.push((bbox, 0, Vec::new()));
      clusters.len() - 1
    });
    let cluster = &mut clusters[slot];
    cluster.0 = cluster.0.union(&bbox);
    match asset {
      Some(asset) => cluster.2.push(asset),
      None => cluster.1 += 1,
    }
  }

  // Partes próximas da mesma figura (subfiguras lado a lado, pedaços de um diagrama).
  let mut merged = true;
  while merged {
    merged = false;
    'outer: for i in 0..clusters.len() {
      for j in i + 1..clusters.len() {
        // Imagens raster são autocontidas: só desenhos vetoriais se juntam por proximidade.
        let vector = clusters[i].1 > 0 && clusters[j].1 > 0;
        if vector && clusters[i].0.expand(MERGE_GAP).intersects(&clusters[j].0) {
          let (bbox, paths, images) = clusters.remove(j);
          let target = &mut clusters[i];
          target.0 = target.0.union(&bbox);
          target.1 += paths;
          target.2.extend(images);
          merged = true;
          break 'outer;
        }
      }
    }
  }

  let mut candidates = Vec::new();
  for (bbox, paths, images) in clusters {
    let big_enough =
      bbox.width() >= 40.0 && bbox.height() >= 25.0 && bbox.area() >= page_area * 0.01;
    if !big_enough {
      continue;
    }
    // Em volta de uma foto/captura o texto é do corpo; só o que está sobre ela é rótulo.
    let labels: Vec<&Line> = if paths == 0 {
      page.lines.iter().filter(|l| inside(l, &bbox)).collect()
    } else {
      labels_of(&page.lines, bbox)
    };
    let label_chars: usize = labels.iter().map(|l| l.char_count()).sum();
    // Moldura ou fundo atrás de um bloco de texto (quadro, listagem): o texto fica como texto.
    let text_box = images.is_empty() && paths <= 6 && label_chars > 120;
    // Fundo desenhado atrás de um título (abertura de capítulo, título de slide).
    let decorated_heading = images.is_empty() && labels.iter().any(|l| is_heading(l, body));
    if text_box || decorated_heading {
      continue;
    }
    let labels: Vec<&Line> = labels
      .into_iter()
      .filter(|l| !is_heading(l, body))
      .collect();
    // Rótulos que transbordam a borda do desenho entram no recorte.
    let bbox = labels.iter().fold(bbox, |acc, l| acc.union(&l.bbox));
    let raster = (paths == 0 && images.len() == 1 && labels.is_empty()).then(|| images[0]);
    candidates.push(Candidate { bbox, raster });
  }
  candidates.sort_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
  candidates
}

fn overlap_share(line: &Line, figure: &Rect) -> f32 {
  let b = &line.bbox;
  let w = (b.x1.min(figure.x1) - b.x0.max(figure.x0)).max(0.0);
  let h = (b.y1.min(figure.y1) - b.y0.max(figure.y0)).max(0.0);
  if b.area() > 0.0 {
    w * h / b.area()
  } else {
    0.0
  }
}

/// A linha está (quase toda) dentro da figura.
fn inside(line: &Line, figure: &Rect) -> bool {
  overlap_share(line, figure) >= 0.6
}

/// Linhas que pertencem à figura: as que estão em boa parte dentro dela e rótulos curtos
/// encostados na borda (que podem, por sua vez, encostar em outros rótulos).
fn labels_of(lines: &[Line], figure: Rect) -> Vec<&Line> {
  let mut bbox = figure;
  let mut taken = vec![false; lines.len()];
  loop {
    let mut grew = false;
    for (i, line) in lines.iter().enumerate() {
      if taken[i] {
        continue;
      }
      let mostly_inside = overlap_share(line, &bbox) >= 0.3;
      let short_label = line.char_count() <= LABEL_CHARS && line.bbox.intersects(&bbox.expand(6.0));
      if mostly_inside || short_label {
        taken[i] = true;
        bbox = bbox.union(&line.bbox);
        grew = true;
      }
    }
    if !grew {
      break;
    }
  }
  lines
    .iter()
    .zip(taken)
    .filter(|(_, t)| *t)
    .map(|(l, _)| l)
    .collect()
}

/// Legenda logo abaixo (ou acima) da figura: "Figura 1.2 ...", "Fig. 3", "Figure 4".
fn caption(lines: &[Line], figure: &Rect) -> Option<String> {
  lines
    .iter()
    .filter(|l| {
      let below = l.bbox.y0 >= figure.y1 && l.bbox.y0 - figure.y1 < l.size * 6.0;
      let above = l.bbox.y1 <= figure.y0 && figure.y0 - l.bbox.y1 < l.size * 3.0;
      (below || above) && l.bbox.x1 > figure.x0 && l.bbox.x0 < figure.x1
    })
    .map(Line::text)
    .find(|t| {
      let lower = t.to_lowercase();
      [
        "figura",
        "fig.",
        "fig ",
        "figure",
        "imagem",
        "image",
        "ilustração",
        "gráfico",
      ]
      .iter()
      .any(|p| lower.starts_with(p))
    })
    .map(|t| t.chars().take(160).collect())
}

fn render_page(document: &PdfDocument, number: u32) -> Option<image::DynamicImage> {
  let page = document.pages().get(number as i32 - 1).ok()?;
  let config = PdfRenderConfig::new().scale_page_by_factor(RENDER_SCALE);
  page.render_with_config(&config).ok()?.as_image().ok()
}

/// Recorte em pixels, com uma pequena margem, limitado à imagem.
fn crop_rect(bbox: &Rect, width: u32, height: u32) -> (u32, u32, u32, u32) {
  let pad = 3.0;
  let x0 = ((bbox.x0 - pad) * RENDER_SCALE).max(0.0) as u32;
  let y0 = ((bbox.y0 - pad) * RENDER_SCALE).max(0.0) as u32;
  let x1 = (((bbox.x1 + pad) * RENDER_SCALE) as u32).min(width);
  let y1 = (((bbox.y1 + pad) * RENDER_SCALE) as u32).min(height);
  (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::{Graphic, Span, Style};

  fn path(x0: f32, y0: f32, x1: f32, y1: f32) -> Graphic {
    Graphic {
      kind: GraphicKind::Path { filled: false },
      bbox: Rect::new(x0, y0, x1, y1),
    }
  }

  fn text(t: &str, x0: f32, y0: f32) -> Line {
    let bbox = Rect::new(x0, y0, x0 + t.len() as f32 * 5.0, y0 + 10.0);
    Line {
      spans: vec![Span {
        text: t.into(),
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
  fn groups_touching_strokes_into_one_figure() {
    // Um diagrama: duas caixas ligadas por uma seta, com rótulos.
    let p = page(
      vec![
        text("Cliente", 110.0, 210.0),
        text("Servidor", 310.0, 210.0),
        text("Figura 1.1 Arquitetura.", 100.0, 250.0),
      ],
      vec![
        path(100.0, 200.0, 200.0, 240.0),
        path(200.0, 219.0, 300.0, 221.0),
        path(300.0, 200.0, 400.0, 240.0),
      ],
    );
    let found = detect(&p, &Ignore::default(), &[], 10.0);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].bbox.x0, 100.0);
    assert!(found[0].raster.is_none());
    assert_eq!(
      caption(&p.lines, &found[0].bbox).as_deref(),
      Some("Figura 1.1 Arquitetura.")
    );
  }

  #[test]
  fn ignores_rules_small_marks_and_framed_text() {
    let paragraph: Vec<Line> = (0..6)
      .map(|i| {
        text(
          "Texto dentro de um quadro sombreado, bem comprido.",
          105.0,
          410.0 + i as f32 * 14.0,
        )
      })
      .collect();
    let p = page(
      paragraph,
      vec![
        path(72.0, 100.0, 540.0, 100.5),  // filete
        path(80.0, 150.0, 90.0, 160.0),   // marcador
        path(100.0, 400.0, 500.0, 500.0), // fundo do quadro
      ],
    );
    assert!(detect(&p, &Ignore::default(), &[], 10.0).is_empty());
  }

  #[test]
  fn drawn_background_behind_a_title_is_not_a_figure() {
    let mut title = text("Capítulo 1", 120.0, 150.0);
    title.size = 24.0;
    let p = page(
      vec![title],
      vec![
        path(100.0, 100.0, 500.0, 220.0),
        path(110.0, 110.0, 490.0, 210.0),
      ],
    );
    assert!(detect(&p, &Ignore::default(), &[], 10.0).is_empty());
  }

  #[test]
  fn repeated_template_shapes_are_dropped() {
    let band = |n| Candidate {
      bbox: Rect::new(0.0, 700.0 + n as f32 * 0.2, 600.0, 760.0),
      raster: None,
    };
    let unique = Candidate {
      bbox: Rect::new(100.0, 100.0, 300.0, 200.0),
      raster: None,
    };
    let mut pages = vec![vec![band(0), unique], vec![band(1)], vec![band(2)]];
    drop_repeated(&mut pages);
    assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), [1, 0, 0]);
  }

  #[test]
  fn single_image_uses_original_file_unless_decorative() {
    let image = Graphic {
      kind: GraphicKind::Image { asset: 0 },
      bbox: Rect::new(100.0, 100.0, 400.0, 300.0),
    };
    let p = page(vec![], vec![image]);
    assert_eq!(detect(&p, &Ignore::default(), &[], 10.0)[0].raster, Some(0));
    assert!(
      detect(
        &p,
        &Ignore {
          images: HashSet::from([0]),
          ..Ignore::default()
        },
        &[],
        10.0
      )
      .is_empty()
    );
  }
}

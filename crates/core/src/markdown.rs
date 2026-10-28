//! Renderização dos capítulos em Markdown (CommonMark, como o mdBook espera).
//!
//! O escape é mínimo: um caractere só é escapado quando de fato formaria sintaxe, para o `.md`
//! continuar legível como texto.

use crate::model::Style;
use crate::structure::{Block, Chapter, Inline};
use crate::text::ListKind;

/// Diretório das imagens, relativo a `src/` (onde ficam os capítulos).
pub const IMAGES_DIR: &str = "images";

/// Filtro do plugin image-darkmodifier do Obsidian, posto em todas as imagens: no tema escuro
/// inverte a imagem e remove o fundo; no claro (modo "theme aware") mantém as cores.
pub const DARKMODE_FILTER: &str = "@darkmode";

pub fn render_chapter(chapter: &Chapter) -> String {
  render_chapter_at(chapter, 1)
}

/// Renderiza o capítulo com o título no nível `base` (`#` = 1); os subtítulos descem junto.
/// O livro inteiro num arquivo só usa `base` = profundidade do capítulo.
pub fn render_chapter_at(chapter: &Chapter, base: u8) -> String {
  let base = base.clamp(1, 5);
  let mut out = format!(
    "{} {}\n\n",
    "#".repeat(base as usize),
    escape_heading(&chapter.title)
  );
  let mut in_list = false;
  let mut notes = Vec::new();
  for block in &chapter.blocks {
    let is_item = matches!(block, Block::ListItem { .. });
    if in_list && !is_item && !matches!(block, Block::Footnote { .. }) {
      out.push('\n');
    }
    if !matches!(block, Block::Footnote { .. }) {
      in_list = is_item;
    }
    match block {
      Block::Code(code) => {
        let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(longest.max(2) + 1);
        out.push_str(&format!("{fence}\n{code}\n{fence}\n\n"));
      }
      Block::Footnote { id, content } => notes.push((id, content)),
      Block::Table(rows) => out.push_str(&render_table(rows)),
      Block::Image { file, alt } => {
        // O plugin image-darkmodifier escurece as imagens marcadas com o filtro no texto alternativo.
        let alt = escape_link_text(alt);
        let alt = if alt.is_empty() {
          DARKMODE_FILTER.to_string()
        } else {
          format!("{alt} {DARKMODE_FILTER}")
        };
        out.push_str(&format!("![{alt}]({IMAGES_DIR}/{file})\n\n"));
      }
      Block::Heading { level, text } => {
        let hashes = (level.saturating_sub(chapter.depth) + base).clamp(base + 1, 6) as usize;
        out.push_str(&format!(
          "{} {}\n\n",
          "#".repeat(hashes),
          escape_heading(text)
        ));
      }
      Block::Paragraph(content) => {
        out.push_str(&render_paragraph(content));
        out.push_str("\n\n");
      }
      Block::ListItem {
        kind,
        depth,
        content,
      } => {
        let indent = "    ".repeat(*depth as usize);
        let body = render_paragraph(content);
        match kind {
          ListKind::Bullet => out.push_str(&format!("{indent}- {body}\n")),
          ListKind::Number(n) => out.push_str(&format!("{indent}{n}. {body}\n")),
          ListKind::Label(label) => {
            out.push_str(&format!("{indent}- {} {body}\n", escape_text(label)))
          }
        }
      }
    }
  }
  if in_list {
    out.push('\n');
  }
  // Notas de rodapé no fim do capítulo, como o mdBook as exibe.
  for (id, content) in notes {
    out.push_str(&format!("[^{id}]: {}\n\n", render_inlines(content)));
  }
  let trimmed = out.trim_end().len();
  out.truncate(trimmed);
  out.push('\n');
  out
}

/// Tabela no formato do GitHub (suportado pelo mdBook); a primeira linha é o cabeçalho.
fn render_table(rows: &[Vec<Vec<Inline>>]) -> String {
  let cell = |c: &Vec<Inline>| {
    let text = render_inlines(c).replace('|', "\\|");
    if text.is_empty() {
      " ".to_string()
    } else {
      text
    }
  };
  let row = |r: &Vec<Vec<Inline>>| {
    format!(
      "| {} |\n",
      r.iter().map(cell).collect::<Vec<_>>().join(" | ")
    )
  };
  let mut out = row(&rows[0]);
  out.push_str(&format!("|{}\n", " --- |".repeat(rows[0].len())));
  for r in &rows[1..] {
    out.push_str(&row(r));
  }
  out.push('\n');
  out
}

/// Renderiza trechos com estilo, juntando os vizinhos de mesmo estilo.
pub fn render_inlines(content: &[Inline]) -> String {
  enum Run {
    Text(String, Style),
    /// Markdown/HTML já pronto (sobrescrito, referência de nota).
    Raw(String),
    Break,
  }
  let mut runs: Vec<Run> = Vec::new();
  for inline in content {
    match inline {
      Inline::Text { text, style } => match runs.last_mut() {
        Some(Run::Text(prev, prev_style)) if prev_style == style => prev.push_str(text),
        _ => runs.push(Run::Text(text.clone(), *style)),
      },
      Inline::Break => runs.push(Run::Break),
      Inline::Sup(t) => runs.push(Run::Raw(format!("<sup>{}</sup>", escape_text(t)))),
      Inline::Sub(t) => runs.push(Run::Raw(format!("<sub>{}</sub>", escape_text(t)))),
      Inline::FootnoteRef(id) => runs.push(Run::Raw(format!("[^{id}]"))),
    }
  }
  let mut out = String::new();
  for run in runs {
    let (text, style) = match run {
      Run::Text(text, style) => (text, style),
      Run::Raw(raw) => {
        out.push_str(&raw);
        continue;
      }
      Run::Break => {
        // Quebra de linha explícita do CommonMark.
        let trimmed = out.trim_end().len();
        out.truncate(trimmed);
        out.push_str("\\\n");
        continue;
      }
    };
    let text: String = text.chars().filter(|&c| c != '\u{ad}').collect();
    let text = if out.ends_with('\n') {
      text.trim_start().to_string()
    } else {
      text
    };
    if style.mono {
      out.push_str(&code_span(&text));
    } else {
      out.push_str(&emphasize(&escape_text(&text), style));
    }
  }
  out
}

/// Renderiza um parágrafo: cada linha (após quebras explícitas) é protegida contra virar bloco.
fn render_paragraph(content: &[Inline]) -> String {
  render_inlines(content)
    .split('\n')
    .map(escape_block_start)
    .collect::<Vec<_>>()
    .join("\n")
}

/// Envolve em `**`/`*`, deixando os espaços das pontas do lado de fora dos marcadores.
fn emphasize(text: &str, style: Style) -> String {
  let marker = match (style.bold, style.italic) {
    (true, true) => "***",
    (true, false) => "**",
    (false, true) => "*",
    (false, false) => return text.to_string(),
  };
  let core = text.trim();
  if core.is_empty() {
    return text.to_string();
  }
  let lead = &text[..text.len() - text.trim_start().len()];
  let trail = &text[text.trim_end().len()..];
  format!("{lead}{marker}{core}{marker}{trail}")
}

fn code_span(text: &str) -> String {
  let core = text.trim();
  if core.is_empty() {
    return text.to_string();
  }
  let lead = &text[..text.len() - text.trim_start().len()];
  let trail = &text[text.trim_end().len()..];
  let longest = core.split(|c| c != '`').map(str::len).max().unwrap_or(0);
  let fence = "`".repeat(longest + 1);
  let pad = if core.starts_with('`') || core.ends_with('`') {
    " "
  } else {
    ""
  };
  format!("{lead}{fence}{pad}{core}{pad}{fence}{trail}")
}

/// Escapa só o que viraria formatação, link, HTML ou diretiva do mdBook.
pub fn escape_text(text: &str) -> String {
  escape(text, false)
}

/// Texto de link (SUMMARY.md): colchetes sempre escapados.
pub fn escape_link_text(text: &str) -> String {
  escape(text, true)
}

fn escape(text: &str, brackets: bool) -> String {
  let chars: Vec<char> = text.chars().collect();
  let mut out = String::with_capacity(text.len());
  for (i, &c) in chars.iter().enumerate() {
    let prev = i.checked_sub(1).map(|j| chars[j]);
    let next = chars.get(i + 1).copied();
    let escape = match c {
      '\\' | '`' | '*' => true,
      // `_` dentro de palavra (snake_case) não forma ênfase.
      '_' => !(prev.is_some_and(char::is_alphanumeric) && next.is_some_and(char::is_alphanumeric)),
      // Só forma HTML/autolink se houver um `>` fechando adiante.
      '<' => {
        next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!' | '?'))
          && chars[i + 1..].contains(&'>')
      }
      '[' | ']' if brackets => true,
      '[' => {
        let rest: String = chars[i + 1..].iter().collect();
        rest.contains("](") || rest.contains("][") || rest.starts_with('^')
      }
      '&' => {
        let rest: String = chars[i + 1..].iter().take(10).collect();
        rest.starts_with('#')
          || rest
            .split_once(';')
            .is_some_and(|(name, _)| !name.is_empty() && name.chars().all(char::is_alphanumeric))
      }
      '~' => prev == Some('~') || next == Some('~'),
      // Diretivas do mdBook: {{#include ...}}.
      '{' => next == Some('{') && chars.get(i + 2) == Some(&'#'),
      _ => false,
    };
    if escape {
      out.push('\\');
    }
    out.push(c);
  }
  out
}

fn escape_heading(text: &str) -> String {
  let escaped = escape_text(text);
  match escaped.strip_suffix('#') {
    Some(rest) => format!("{rest}\\#"),
    None => escaped,
  }
}

/// Evita que o início de um parágrafo vire título, citação, lista ou bloco.
fn escape_block_start(text: &str) -> String {
  let digits = text.chars().take_while(char::is_ascii_digit).count();
  let numbered = digits > 0
    && digits <= 9
    && text[digits..].starts_with(['.', ')'])
    && text[digits + 1..].starts_with([' ', '\t']);
  let marker = text.starts_with(['#', '>', '|'])
    || ((text.starts_with(['+', '-', '=']) || text.starts_with("* "))
      && (text.len() == 1 || text[1..].starts_with([' ', '-', '='])));
  if numbered {
    format!("{}\\{}", &text[..digits], &text[digits..])
  } else if marker {
    format!("\\{text}")
  } else {
    text.to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn chapter(blocks: Vec<Block>) -> Chapter {
    Chapter {
      title: "Capítulo 1".into(),
      depth: 1,
      blocks,
    }
  }

  #[test]
  fn plain_prose_is_left_untouched() {
    let text = "Preço: 10% (aprox.), arquivo meu_arquivo.txt, 3 > 2 e a + b - c = d. Ver [1].";
    assert_eq!(escape_text(text), text);
  }

  #[test]
  fn escapes_only_real_syntax() {
    assert_eq!(escape_text("a *b* c"), "a \\*b\\* c");
    assert_eq!(escape_text("_ênfase_"), "\\_ênfase\\_");
    assert_eq!(escape_text("<div> e x<y"), "\\<div> e x<y");
    assert_eq!(escape_text("[texto](url)"), "\\[texto](url)");
    assert_eq!(escape_text("&amp; e & sozinho"), "\\&amp; e & sozinho");
    assert_eq!(escape_text("{{#include x}}"), "\\{{#include x}}");
    assert_eq!(escape_text("~~riscado~~ e ~5"), "\\~\\~riscado\\~\\~ e ~5");
  }

  #[test]
  fn link_text_always_escapes_brackets() {
    assert_eq!(
      escape_link_text("Array[int] e *x*"),
      "Array\\[int\\] e \\*x\\*"
    );
  }

  #[test]
  fn escapes_block_markers_at_paragraph_start() {
    assert_eq!(escape_block_start("1. não é lista"), "1\\. não é lista");
    assert_eq!(escape_block_start("1.5 é número"), "1.5 é número");
    assert_eq!(escape_block_start("- traço"), "\\- traço");
    assert_eq!(escape_block_start("-5 graus"), "-5 graus");
    assert_eq!(escape_block_start("# não é título"), "\\# não é título");
  }

  #[test]
  fn renders_chapter_with_subheadings_and_lists() {
    let c = chapter(vec![
      Block::Paragraph(vec![Inline::plain("Texto com *asterisco*.")]),
      Block::Heading {
        level: 2,
        text: "Seção".into(),
      },
      Block::ListItem {
        kind: ListKind::Number(1),
        depth: 0,
        content: vec![Inline::plain("um")],
      },
      Block::ListItem {
        kind: ListKind::Bullet,
        depth: 1,
        content: vec![Inline::plain("sub")],
      },
      Block::ListItem {
        kind: ListKind::Label("a)".into()),
        depth: 0,
        content: vec![Inline::plain("letra")],
      },
      Block::Paragraph(vec![Inline::plain("Fim.")]),
    ]);
    assert_eq!(
      render_chapter(&c),
      "# Capítulo 1\n\nTexto com \\*asterisco\\*.\n\n## Seção\n\n1. um\n    - sub\n- a) letra\n\nFim.\n"
    );
  }

  #[test]
  fn chapter_can_start_at_a_deeper_heading_level() {
    let c = Chapter {
      title: "Cap 1".into(),
      depth: 2,
      blocks: vec![Block::Heading {
        level: 3,
        text: "Seção".into(),
      }],
    };
    assert_eq!(render_chapter_at(&c, 2), "## Cap 1\n\n### Seção\n");
  }

  #[test]
  fn subheading_level_is_relative_to_chapter_depth() {
    let c = Chapter {
      title: "Seção".into(),
      depth: 2,
      blocks: vec![Block::Heading {
        level: 3,
        text: "Sub".into(),
      }],
    };
    assert_eq!(render_chapter(&c), "# Seção\n\n## Sub\n");
  }

  #[test]
  fn emphasis_keeps_spaces_outside_markers() {
    let bold = Style {
      bold: true,
      ..Style::default()
    };
    let content = vec![
      Inline::plain("um "),
      Inline::Text {
        text: "negrito ".into(),
        style: bold,
      },
      Inline::plain("aqui"),
    ];
    assert_eq!(render_inlines(&content), "um **negrito** aqui");
  }

  #[test]
  fn verse_lines_use_hard_breaks_and_stay_escaped() {
    let content = vec![
      Inline::plain("primeiro verso"),
      Inline::Break,
      Inline::plain("- segundo verso"),
    ];
    assert_eq!(
      render_paragraph(&content),
      "primeiro verso\\\n\\- segundo verso"
    );
  }

  #[test]
  fn renders_code_blocks_scripts_and_footnotes() {
    let c = chapter(vec![
      Block::Paragraph(vec![
        Inline::plain("O(n"),
        Inline::Sup("2".into()),
        Inline::plain(") e H"),
        Inline::Sub("2".into()),
        Inline::plain("O, ver nota"),
        Inline::FootnoteRef("3-1".into()),
        Inline::plain("."),
      ]),
      Block::Footnote {
        id: "3-1".into(),
        content: vec![Inline::plain("A nota.")],
      },
      Block::Code("fn main() {\n    let x = 1;\n}".into()),
    ]);
    assert_eq!(
      render_chapter(&c),
      "# Capítulo 1\n\nO(n<sup>2</sup>) e H<sub>2</sub>O, ver nota[^3-1].\n\n```\nfn main() {\n    let x = 1;\n}\n```\n\n[^3-1]: A nota.\n"
    );
  }

  #[test]
  fn renders_gfm_tables() {
    let c = chapter(vec![Block::Table(vec![
      vec![vec![Inline::plain("Op")], vec![Inline::plain("Uso")]],
      vec![vec![Inline::plain("|")], vec![]],
    ])]);
    assert_eq!(
      render_chapter(&c),
      "# Capítulo 1\n\n| Op | Uso |\n| --- | --- |\n| \\| |   |\n"
    );
  }

  #[test]
  fn images_get_the_darkmode_filter() {
    let c = chapter(vec![
      Block::Image {
        file: "fig-0001-1.png".into(),
        alt: "Figura 1.1 Arquitetura".into(),
      },
      Block::Image {
        file: "img-0002.jpg".into(),
        alt: String::new(),
      },
    ]);
    assert_eq!(
      render_chapter(&c),
      "# Capítulo 1\n\n![Figura 1.1 Arquitetura @darkmode](images/fig-0001-1.png)\n\n![@darkmode](images/img-0002.jpg)\n"
    );
  }

  #[test]
  fn code_spans_use_long_enough_fences() {
    assert_eq!(code_span("a`b"), "``a`b``");
    assert_eq!(code_span("ls -l "), "`ls -l` ");
  }
}

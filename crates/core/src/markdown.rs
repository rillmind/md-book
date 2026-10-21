//! Renderização dos capítulos em Markdown (CommonMark, como o mdBook espera).

use crate::structure::{Block, Chapter};

pub fn render_chapter(chapter: &Chapter) -> String {
  let mut out = format!("# {}\n\n", escape_inline(&chapter.title));
  let mut in_list = false;
  for block in &chapter.blocks {
    let is_item = matches!(block, Block::ListItem(_));
    if in_list && !is_item {
      out.push('\n');
    }
    in_list = is_item;
    match block {
      Block::Heading { level, text } => {
        let hashes = (level.saturating_sub(chapter.depth) + 1).clamp(2, 6) as usize;
        out.push_str(&format!(
          "{} {}\n\n",
          "#".repeat(hashes),
          escape_inline(text)
        ));
      }
      Block::Paragraph(text) => {
        out.push_str(&escape_block_start(&escape_inline(text)));
        out.push_str("\n\n");
      }
      Block::ListItem(text) => out.push_str(&format!("- {}\n", escape_inline(text))),
    }
  }
  let trimmed = out.trim_end().len();
  out.truncate(trimmed);
  out.push('\n');
  out
}

/// Escapa caracteres que o Markdown interpretaria como formatação ou HTML.
pub fn escape_inline(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  for c in text.chars() {
    if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '|') {
      out.push('\\');
    }
    out.push(c);
  }
  out
}

/// Evita que o início de um parágrafo vire título, citação ou lista.
fn escape_block_start(text: &str) -> String {
  let starts_marker = text.starts_with(['#', '+', '-', '=']);
  let numbered = {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && text[digits..].starts_with(['.', ')'])
  };
  if starts_marker {
    format!("\\{text}")
  } else if numbered {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    format!("{}\\{}", &text[..digits], &text[digits..])
  } else {
    text.to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn renders_chapter_with_subheadings_and_lists() {
    let chapter = Chapter {
      title: "Capítulo 1".into(),
      depth: 1,
      blocks: vec![
        Block::Paragraph("Texto com *asterisco* e <tag>.".into()),
        Block::Heading {
          level: 2,
          text: "Seção".into(),
        },
        Block::ListItem("um".into()),
        Block::ListItem("dois".into()),
        Block::Paragraph("1. não é lista".into()),
      ],
    };
    assert_eq!(
      render_chapter(&chapter),
      "# Capítulo 1\n\n\
             Texto com \\*asterisco\\* e \\<tag\\>.\n\n\
             ## Seção\n\n\
             - um\n\
             - dois\n\n\
             1\\. não é lista\n"
    );
  }

  #[test]
  fn subheading_level_is_relative_to_chapter_depth() {
    let chapter = Chapter {
      title: "Seção".into(),
      depth: 2,
      blocks: vec![Block::Heading {
        level: 3,
        text: "Sub".into(),
      }],
    };
    assert_eq!(render_chapter(&chapter), "# Seção\n\n## Sub\n");
  }
}

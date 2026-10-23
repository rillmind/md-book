//! Regras de texto: junção de linhas com hifenização, marcadores de lista e pontuação.

use std::collections::HashSet;

/// Pronomes oblíquos que seguem hífen em português ("fazê-lo", "disse-lhe"): o hífen é da
/// palavra, não da quebra de linha.
const CLITICS: &[&str] = &[
  "o", "a", "os", "as", "lo", "la", "los", "las", "no", "na", "nos", "nas", "me", "te", "se",
  "lhe", "lhes", "vos", "mo", "ma", "to", "ta", "lho", "lha",
];

/// Palavras que aparecem no livro (fora do fim de linha), para decidir a hifenização.
#[derive(Debug, Default)]
pub struct Vocabulary {
  words: HashSet<String>,
}

impl Vocabulary {
  /// Registra as palavras de uma linha, exceto a última (que pode estar hifenizada).
  pub fn add_line(&mut self, line: &str) {
    let mut words = line.split_whitespace().peekable();
    while let Some(word) = words.next() {
      if words.peek().is_none() {
        break;
      }
      let word = trim_word(word);
      if word.chars().count() > 1 {
        self.words.insert(word.to_lowercase());
      }
    }
  }

  fn contains(&self, word: &str) -> bool {
    self.words.contains(&word.to_lowercase())
  }
}

fn trim_word(word: &str) -> &str {
  word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
}

/// Junta o texto da próxima linha ao parágrafo, decidindo se o hífen final é de quebra de linha
/// ("conti-" + "nua" → "continua") ou da palavra ("bem-" + "estar" → "bem-estar").
pub fn join(text: &mut String, next: &str, vocabulary: &Vocabulary) {
  if text.ends_with('\u{ad}') {
    text.pop();
    text.push_str(next);
    return;
  }
  // Hífen colado a uma palavra no fim da linha: junta sem espaço, removendo o hífen só se ele
  // for de quebra de linha.
  let mut tail = text.chars().rev();
  if tail.next() == Some('-') && tail.next().is_some_and(char::is_alphanumeric) {
    // Tipografia portuguesa repete o hífen da palavra composta na linha seguinte.
    if let Some(rest) = next.strip_prefix('-')
      && rest.starts_with(char::is_alphabetic)
    {
      text.push_str(rest);
      return;
    }
    if should_dehyphenate(text, next, vocabulary) {
      text.pop();
    }
    text.push_str(next);
    return;
  }
  if !text.is_empty()
    && !text.ends_with(char::is_whitespace)
    && !next.starts_with(char::is_whitespace)
  {
    text.push(' ');
  }
  text.push_str(next);
}

fn should_dehyphenate(text: &str, next: &str, vocabulary: &Vocabulary) -> bool {
  let before = &text[..text.len() - 1];
  let left: String = before
    .chars()
    .rev()
    .take_while(|c| c.is_alphabetic())
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect();
  let right: String = next.chars().take_while(|c| c.is_alphabetic()).collect();
  if left.is_empty() || right.is_empty() || !right.starts_with(char::is_lowercase) {
    return false;
  }
  if CLITICS.contains(&right.as_str())
    && next[right.len()..].starts_with(|c: char| !c.is_alphabetic() || c == '-')
  {
    return false;
  }
  let joined = format!("{left}{right}");
  if vocabulary.contains(&joined) {
    return true;
  }
  if vocabulary.contains(&format!("{left}-{right}")) {
    return false;
  }
  // Sem evidência, a quebra tipográfica é bem mais comum que palavra composta.
  true
}

/// Remove hífens invisíveis que não ficaram no fim da linha.
pub fn strip_soft_hyphens(text: &str) -> String {
  let trimmed = text.trim_end_matches('\u{ad}');
  let mut out: String = trimmed.chars().filter(|&c| c != '\u{ad}').collect();
  if trimmed.len() != text.len() {
    out.push('\u{ad}');
  }
  out
}

/// A linha seguinte continua a frase da anterior: hífen no fim, ou frase sem pontuação final
/// seguida de minúscula. Vale mais que a geometria (recuos e espaços variam muito).
pub fn continues_sentence(prev: &str, next: &str) -> bool {
  let prev = prev.trim_end();
  if prev.ends_with('\u{ad}')
    || (prev.ends_with('-') && prev.chars().rev().nth(1).is_some_and(char::is_alphanumeric))
  {
    return true;
  }
  !ends_sentence(prev) && next.trim_start().starts_with(char::is_lowercase)
}

pub fn ends_sentence(text: &str) -> bool {
  let t = text
    .trim_end()
    .trim_end_matches(['"', '”', '’', ')', '»', '*', '_']);
  t.ends_with(['.', '!', '?', ':', '…'])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListKind {
  Bullet,
  /// Numeração com algarismos: vira lista ordenada do Markdown.
  Number(u32),
  /// Letras ou romanos ("a)", "iv."): preservados no texto do item.
  Label(String),
}

/// Reconhece um marcador de lista no início da linha e devolve o restante do texto.
pub fn list_marker(text: &str) -> Option<(ListKind, &str)> {
  let mut chars = text.char_indices();
  let (_, first) = chars.next()?;
  let after_first = &text[first.len_utf8()..];

  let bullet = matches!(
    first,
    '•'
      | '●'
      | '▪'
      | '■'
      | '◦'
      | '○'
      | '‣'
      | '⁃'
      | '✓'
      | '✔'
      | '➢'
      | '➤'
      | '►'
      | '▶'
      | '❖'
      | '◆'
  ) || ('\u{f000}'..='\u{f0ff}').contains(&first)
    || (matches!(first, '-' | '*' | '·') && after_first.starts_with(' '));
  if bullet {
    let rest = after_first.trim_start();
    return (!rest.is_empty()).then_some((ListKind::Bullet, rest));
  }

  // "1." "12)" "(3)" "a)" "iv." — sempre seguido de espaço e de texto.
  let inner_start = usize::from(first == '(');
  let body = &text[inner_start..];
  let label_len = body
    .char_indices()
    .find(|(_, c)| !c.is_alphanumeric())
    .map(|(i, _)| i)?;
  let label = &body[..label_len];
  let closer = body[label_len..].chars().next()?;
  let valid_closer = if inner_start == 1 {
    closer == ')'
  } else {
    matches!(closer, '.' | ')')
  };
  let rest = &body[label_len + closer.len_utf8()..];
  if !valid_closer || !rest.starts_with(' ') {
    return None;
  }
  let rest = rest.trim_start();
  // "1.2 Título" ou "3. 14" não são listas.
  if rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_digit()) {
    return None;
  }
  if label.len() <= 3 && label.chars().all(|c| c.is_ascii_digit()) {
    return Some((ListKind::Number(label.parse().ok()?), rest));
  }
  // Com ponto, só minúsculas: "A. Silva" ou "I. Newton" são nomes, não itens.
  let case_ok = closer == ')' || label.chars().all(|c| c.is_ascii_lowercase());
  let roman = label.len() <= 5 && label.chars().all(|c| "ivxlcIVXLC".contains(c));
  let letter = label.chars().count() == 1 && label.chars().all(|c| c.is_ascii_alphabetic());
  if case_ok && (roman || letter) {
    let marker = &text[..inner_start + label_len + closer.len_utf8()];
    return Some((ListKind::Label(marker.to_string()), rest));
  }
  None
}

/// Forma canônica para comparar títulos: só letras e números ASCII, minúsculos.
pub fn normalize(text: &str) -> String {
  deunicode::deunicode(text)
    .chars()
    .filter(|c| c.is_ascii_alphanumeric())
    .map(|c| c.to_ascii_lowercase())
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn vocab(lines: &[&str]) -> Vocabulary {
    let mut v = Vocabulary::default();
    for l in lines {
      v.add_line(l);
    }
    v
  }

  fn joined(a: &str, b: &str, v: &Vocabulary) -> String {
    let mut s = a.to_string();
    join(&mut s, b, v);
    s
  }

  #[test]
  fn dehyphenates_line_breaks() {
    let v = Vocabulary::default();
    assert_eq!(
      joined("um parágrafo que conti-", "nua aqui", &v),
      "um parágrafo que continua aqui"
    );
    assert_eq!(
      joined("texto com hífen\u{ad}", "ização", &v),
      "texto com hífenização"
    );
  }

  #[test]
  fn keeps_compound_words_and_clitics() {
    let v = vocab(&["o bem-estar de todos importa", "nada"]);
    assert_eq!(
      joined("pelo bem-", "estar geral", &v),
      "pelo bem-estar geral"
    );
    assert_eq!(
      joined("era preciso fazê-", "lo logo", &Vocabulary::default()),
      "era preciso fazê-lo logo"
    );
    assert_eq!(
      joined("disse-", "lhe que sim", &Vocabulary::default()),
      "disse-lhe que sim"
    );
    assert_eq!(
      joined("o código-", "-fonte", &Vocabulary::default()),
      "o código-fonte"
    );
  }

  #[test]
  fn prefers_word_seen_in_the_book() {
    let v = vocab(&["a microcomputação mudou tudo"]);
    assert_eq!(joined("a micro-", "computação", &v), "a microcomputação");
  }

  #[test]
  fn does_not_join_uppercase_or_numbers() {
    let v = Vocabulary::default();
    assert_eq!(joined("pré-", "Socrático", &v), "pré-Socrático");
    assert_eq!(joined("páginas 10-", "20", &v), "páginas 10-20");
    assert_eq!(
      joined("um travessão -", "e segue", &v),
      "um travessão - e segue"
    );
  }

  #[test]
  fn detects_list_markers() {
    assert_eq!(list_marker("• item"), Some((ListKind::Bullet, "item")));
    assert_eq!(
      list_marker("\u{f0b7} item"),
      Some((ListKind::Bullet, "item"))
    );
    assert_eq!(list_marker("- item"), Some((ListKind::Bullet, "item")));
    assert_eq!(
      list_marker("3. terceiro"),
      Some((ListKind::Number(3), "terceiro"))
    );
    assert_eq!(
      list_marker("12) doze"),
      Some((ListKind::Number(12), "doze"))
    );
    assert_eq!(
      list_marker("a) letra"),
      Some((ListKind::Label("a)".into()), "letra"))
    );
    assert_eq!(
      list_marker("(iv) romano"),
      Some((ListKind::Label("(iv)".into()), "romano"))
    );
    assert_eq!(list_marker("1.2 Título"), None);
    assert_eq!(list_marker("3. 14 é pi"), None);
    assert_eq!(list_marker("-5 graus"), None);
    assert_eq!(list_marker("Era. Assim"), None);
    assert_eq!(list_marker("A. Silva disse"), None);
    assert_eq!(list_marker("— Quem é você?"), None);
    assert_eq!(
      list_marker("B) opção"),
      Some((ListKind::Label("B)".into()), "opção"))
    );
  }

  #[test]
  fn sentence_endings() {
    assert!(ends_sentence("Fim."));
    assert!(ends_sentence("Fim.”"));
    assert!(!ends_sentence("continua"));
  }
}

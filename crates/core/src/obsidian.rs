//! Configurações padrão do Obsidian (temas, plugins, atalhos, snippets) embutidas no binário e
//! aplicadas à pasta `.obsidian` de cada livro gerado. O snapshot vem de
//! `scripts/atualizar-obsidian.sh`.

use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, DirEntry, include_dir};

use crate::Error;

static TEMPLATE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/obsidian");

/// Pasta de configuração do vault.
pub const CONFIG_DIR: &str = ".obsidian";

/// Resultado de conferir (e, se pedido, aplicar) as configurações num diretório.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigReport {
  /// Arquivos do padrão que não existiam (caminhos relativos a `.obsidian`).
  pub missing: Vec<PathBuf>,
  /// Arquivos que existiam com conteúdo diferente do padrão.
  pub outdated: Vec<PathBuf>,
}

impl ConfigReport {
  pub fn is_up_to_date(&self) -> bool {
    self.missing.is_empty() && self.outdated.is_empty()
  }
}

/// Confere se `dir/.obsidian` tem todas as configurações padrão, sem alterar nada.
pub fn check(dir: &Path) -> ConfigReport {
  let mut report = ConfigReport::default();
  for (relative, contents) in files() {
    match fs::read(dir.join(CONFIG_DIR).join(&relative)) {
      Ok(current) if current == contents => {}
      Ok(_) => report.outdated.push(relative),
      Err(_) => report.missing.push(relative),
    }
  }
  report
}

/// Garante as configurações padrão em `dir/.obsidian`: cria o que falta e restaura o que difere.
/// Arquivos que não fazem parte do padrão (plugins extras, estado da janela) ficam intactos.
pub fn apply(dir: &Path) -> Result<ConfigReport, Error> {
  let report = check(dir);
  let config = dir.join(CONFIG_DIR);
  for relative in report.missing.iter().chain(&report.outdated) {
    let path = config.join(relative);
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    let contents = TEMPLATE
      .get_file(relative)
      .map(|f| f.contents())
      .unwrap_or_default();
    fs::write(path, contents)?;
  }
  Ok(report)
}

/// Todos os arquivos do padrão, com caminho relativo a `.obsidian`.
fn files() -> Vec<(PathBuf, &'static [u8])> {
  fn walk(dir: &'static Dir<'static>, out: &mut Vec<(PathBuf, &'static [u8])>) {
    for entry in dir.entries() {
      match entry {
        DirEntry::Dir(sub) => walk(sub, out),
        DirEntry::File(file) => out.push((file.path().to_path_buf(), file.contents())),
      }
    }
  }
  let mut out = Vec::new();
  walk(&TEMPLATE, &mut out);
  out.sort();
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("md-book-obsidian-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn template_has_the_vault_settings() {
    let names: Vec<String> = files()
      .into_iter()
      .map(|(p, _)| p.display().to_string())
      .collect();
    for expected in [
      "app.json",
      "hotkeys.json",
      "community-plugins.json",
      "appearance.json",
    ] {
      assert!(
        names.contains(&expected.to_string()),
        "{expected} ausente em {names:?}"
      );
    }
    assert!(names.iter().any(|n| n.starts_with("plugins/")));
    assert!(
      !names
        .iter()
        .any(|n| n.contains("workspace") || n.contains("sync-conflict"))
    );
  }

  #[test]
  fn apply_creates_restores_and_keeps_extra_files() {
    let dir = temp("apply");
    assert_eq!(check(&dir).missing.len(), files().len());

    let first = apply(&dir).unwrap();
    assert_eq!(first.missing.len(), files().len());
    assert!(check(&dir).is_up_to_date());

    // O usuário mexeu num atalho e instalou um plugin a mais.
    fs::write(dir.join(".obsidian/hotkeys.json"), "{}").unwrap();
    fs::create_dir_all(dir.join(".obsidian/plugins/extra")).unwrap();
    fs::write(dir.join(".obsidian/plugins/extra/main.js"), "//").unwrap();
    fs::write(dir.join(".obsidian/workspace.json"), "{}").unwrap();

    let report = check(&dir);
    assert_eq!(report.outdated, [PathBuf::from("hotkeys.json")]);
    let second = apply(&dir).unwrap();
    assert_eq!(second.outdated, [PathBuf::from("hotkeys.json")]);
    assert!(check(&dir).is_up_to_date());
    assert!(dir.join(".obsidian/plugins/extra/main.js").exists());
    assert!(dir.join(".obsidian/workspace.json").exists());
    fs::remove_dir_all(&dir).unwrap();
  }
}

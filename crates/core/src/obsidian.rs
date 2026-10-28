//! Configurações padrão do Obsidian (temas, plugins, atalhos, snippets) aplicadas à pasta
//! `.obsidian` de cada livro gerado. O padrão é o embutido no binário (snapshot feito por
//! `scripts/atualizar-obsidian.sh`) ou, se o usuário registrou o seu com `--save-config`, o de
//! `~/.config/md-book` (ver [`crate::config`]).

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{Dir, DirEntry, include_dir};

use crate::{Error, config};

static TEMPLATE: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/obsidian");

/// Pasta de configuração do vault.
pub const CONFIG_DIR: &str = ".obsidian";

/// Arquivos de um padrão: caminho relativo a `.obsidian` e conteúdo.
type TemplateFiles = Vec<(PathBuf, Cow<'static, [u8]>)>;

/// De onde vêm as configurações padrão.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
  /// Embutidas no binário.
  Embedded,
  /// Registradas pelo usuário (`md-book --save-config`) neste diretório.
  User(PathBuf),
}

impl Template {
  /// O padrão do usuário, se ele registrou um; senão, o embutido.
  pub fn detect() -> Self {
    match config::config_dir() {
      Some(dir) if dir.join("config.toml").is_file() => Template::User(dir),
      _ => Template::Embedded,
    }
  }

  /// Arquivos do padrão, com caminho relativo a `.obsidian`.
  fn files(&self) -> Result<TemplateFiles, Error> {
    match self {
      Template::Embedded => Ok(
        embedded()
          .into_iter()
          .map(|(p, bytes)| (p, Cow::Borrowed(bytes)))
          .collect(),
      ),
      Template::User(dir) => {
        let user = config::load(dir)?.unwrap_or_default();
        Ok(
          config::user_files(dir, &user)?
            .into_iter()
            .map(|(p, bytes)| (p, Cow::Owned(bytes)))
            .collect(),
        )
      }
    }
  }
}

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

/// Confere se `dir/.obsidian` tem todas as configurações do padrão, sem alterar nada.
pub fn check(dir: &Path, template: &Template) -> Result<ConfigReport, Error> {
  Ok(compare(dir, &template.files()?))
}

fn compare(dir: &Path, files: &[(PathBuf, Cow<'static, [u8]>)]) -> ConfigReport {
  let mut report = ConfigReport::default();
  for (relative, contents) in files {
    match fs::read(dir.join(CONFIG_DIR).join(relative)) {
      Ok(current) if current == contents.as_ref() => {}
      Ok(_) => report.outdated.push(relative.clone()),
      Err(_) => report.missing.push(relative.clone()),
    }
  }
  report
}

/// Garante as configurações do padrão em `dir/.obsidian`: cria o que falta e restaura o que
/// difere. Arquivos que não fazem parte do padrão (plugins extras, estado da janela) ficam.
pub fn apply(dir: &Path, template: &Template) -> Result<ConfigReport, Error> {
  let files = template.files()?;
  let report = compare(dir, &files);
  let config = dir.join(CONFIG_DIR);
  for (relative, contents) in &files {
    if !report.missing.contains(relative) && !report.outdated.contains(relative) {
      continue;
    }
    let path = config.join(relative);
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    fs::write(path, contents)?;
  }
  Ok(report)
}

/// Arquivos do padrão embutido, com caminho relativo a `.obsidian`.
fn embedded() -> Vec<(PathBuf, &'static [u8])> {
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
    let names: Vec<String> = embedded()
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
    let template = Template::Embedded;
    let total = embedded().len();
    assert_eq!(check(&dir, &template).unwrap().missing.len(), total);

    let first = apply(&dir, &template).unwrap();
    assert_eq!(first.missing.len(), total);
    assert!(check(&dir, &template).unwrap().is_up_to_date());

    // O usuário mexeu num atalho e instalou um plugin a mais.
    fs::write(dir.join(".obsidian/hotkeys.json"), "{}").unwrap();
    fs::create_dir_all(dir.join(".obsidian/plugins/extra")).unwrap();
    fs::write(dir.join(".obsidian/plugins/extra/main.js"), "//").unwrap();
    fs::write(dir.join(".obsidian/workspace.json"), "{}").unwrap();

    let report = check(&dir, &template).unwrap();
    assert_eq!(report.outdated, [PathBuf::from("hotkeys.json")]);
    let second = apply(&dir, &template).unwrap();
    assert_eq!(second.outdated, [PathBuf::from("hotkeys.json")]);
    assert!(check(&dir, &template).unwrap().is_up_to_date());
    assert!(dir.join(".obsidian/plugins/extra/main.js").exists());
    assert!(dir.join(".obsidian/workspace.json").exists());
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn user_template_replaces_the_embedded_one() {
    let config = temp("user-config");
    fs::create_dir_all(config.join("obsidian/plugins/colunas-fixas")).unwrap();
    fs::write(config.join("obsidian/hotkeys.json"), "{\"meu\": []}").unwrap();
    fs::write(config.join("obsidian/plugins/colunas-fixas/main.js"), "//").unwrap();
    fs::write(config.join("config.toml"), "").unwrap();

    let book = temp("user-book");
    let report = apply(&book, &Template::User(config.clone())).unwrap();
    assert_eq!(report.missing.len(), 2);
    assert_eq!(
      fs::read_to_string(book.join(".obsidian/hotkeys.json")).unwrap(),
      "{\"meu\": []}"
    );
    assert!(
      !book.join(".obsidian/app.json").exists(),
      "o padrão embutido não entra"
    );
    fs::remove_dir_all(&config).unwrap();
    fs::remove_dir_all(&book).unwrap();
  }
}

//! Configuração do usuário (`~/.config/md-book`): o padrão do Obsidian registrado a partir de um
//! vault, em formato que dá para guardar nos dotfiles.
//!
//! - `config.toml` referencia plugins e temas da comunidade (id, repositório, versão); eles são
//!   baixados do GitHub e guardados em `~/.cache/md-book`.
//! - `obsidian/` guarda, como estão, os arquivos que não vêm da comunidade: configurações
//!   (`app.json`, `hotkeys.json`...), snippets, configurações de cada plugin (`data.json`) e plugins
//!   ou temas locais, que não estão no catálogo.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Error;

/// Variável de ambiente com o diretório de configuração (substitui `~/.config/md-book`).
pub const CONFIG_ENV: &str = "MD_BOOK_CONFIG";
/// Variável de ambiente com o diretório de cache (substitui `~/.cache/md-book`).
pub const CACHE_ENV: &str = "MD_BOOK_CACHE";

const CONFIG_FILE: &str = "config.toml";
/// `config.toml` do padrão embutido (gerado por `scripts/atualizar-obsidian.sh`).
const DEFAULT_CONFIG: &str = include_str!("../obsidian-config.toml");
const FILES_DIR: &str = "obsidian";
const PLUGIN_CATALOG: &str =
  "https://raw.githubusercontent.com/obsidianmd/obsidian-releases/master/community-plugins.json";
const THEME_CATALOG: &str =
  "https://raw.githubusercontent.com/obsidianmd/obsidian-releases/master/community-css-themes.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserConfig {
  #[serde(default, rename = "plugin", skip_serializing_if = "Vec::is_empty")]
  pub plugins: Vec<PluginRef>,
  #[serde(default, rename = "theme", skip_serializing_if = "Vec::is_empty")]
  pub themes: Vec<ThemeRef>,
}

/// Plugin da comunidade, baixado do release `version` de `repo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginRef {
  pub id: String,
  pub repo: String,
  pub version: String,
}

/// Tema da comunidade, baixado do ramo principal de `repo` (é assim que o Obsidian instala temas).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeRef {
  pub name: String,
  pub repo: String,
}

/// Diretório de configuração: `$MD_BOOK_CONFIG`, `$XDG_CONFIG_HOME/md-book` ou `~/.config/md-book`.
pub fn config_dir() -> Option<PathBuf> {
  env_dir(CONFIG_ENV, "XDG_CONFIG_HOME", ".config")
}

/// Diretório de cache: `$MD_BOOK_CACHE`, `$XDG_CACHE_HOME/md-book` ou `~/.cache/md-book`.
pub fn cache_dir() -> Option<PathBuf> {
  env_dir(CACHE_ENV, "XDG_CACHE_HOME", ".cache")
}

fn env_dir(own: &str, xdg: &str, home_sub: &str) -> Option<PathBuf> {
  let non_empty = |v: std::ffi::OsString| (!v.is_empty()).then(|| PathBuf::from(v));
  if let Some(dir) = std::env::var_os(own).and_then(non_empty) {
    return Some(dir);
  }
  if let Some(base) = std::env::var_os(xdg).and_then(non_empty) {
    return Some(base.join("md-book"));
  }
  std::env::var_os("HOME")
    .and_then(non_empty)
    .map(|home| home.join(home_sub).join("md-book"))
}

/// Lê a configuração do usuário em `dir`, se houver.
pub fn load(dir: &Path) -> Result<Option<UserConfig>, Error> {
  let path = dir.join(CONFIG_FILE);
  if !path.is_file() {
    return Ok(None);
  }
  let text = fs::read_to_string(&path)?;
  toml::from_str(&text)
    .map(Some)
    .map_err(|e| Error::Config(format!("{}: {e}", path.display())))
}

/// Arquivos do padrão do usuário, com caminho relativo a `.obsidian`: o que está em `obsidian/`
/// mais os plugins e temas da comunidade (do cache, ou baixados agora).
pub fn user_files(dir: &Path, config: &UserConfig) -> Result<Vec<(PathBuf, Vec<u8>)>, Error> {
  let mut files: HashMap<PathBuf, Vec<u8>> = HashMap::new();
  let base = dir.join(FILES_DIR);
  if base.is_dir() {
    for path in walk(&base)? {
      let relative = path.strip_prefix(&base).unwrap_or(&path).to_path_buf();
      files.insert(relative, fs::read(&path)?);
    }
  }
  // Sem rede (ou com o GitHub fora do ar), a cópia embutida serve se for a mesma versão.
  for plugin in &config.plugins {
    let fetched = fetch_plugin(plugin).or_else(|e| embedded_plugin(plugin).ok_or(e))?;
    for (name, bytes) in fetched {
      files.insert(Path::new("plugins").join(&plugin.id).join(name), bytes);
    }
  }
  for theme in &config.themes {
    let fetched = fetch_theme(theme).or_else(|e| embedded_theme(theme).ok_or(e))?;
    for (name, bytes) in fetched {
      files.insert(Path::new("themes").join(&theme.name).join(name), bytes);
    }
  }
  let mut out: Vec<(PathBuf, Vec<u8>)> = files.into_iter().collect();
  out.sort();
  Ok(out)
}

/// Cria a configuração do usuário em `dir` com o padrão embutido, se ainda não houver
/// `config.toml`. Devolve se criou.
pub fn ensure_default(dir: &Path) -> Result<bool, Error> {
  if dir.join(CONFIG_FILE).is_file() {
    return Ok(false);
  }
  let config = default_config()?;
  let files_dir = dir.join(FILES_DIR);
  for (relative, bytes) in crate::obsidian::embedded() {
    if is_referenced(&config, &relative) {
      continue;
    }
    let path = files_dir.join(&relative);
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
  }
  let header = "# Padrão do Obsidian para os livros gerados pelo md-book.\n\
                # Criado com o padrão embutido; para registrar o seu: `md-book --save-config <vault>`.\n\
                # Plugins e temas abaixo são baixados da comunidade; o resto está em obsidian/.\n\n";
  let body = DEFAULT_CONFIG
    .lines()
    .skip_while(|l| l.starts_with('#'))
    .collect::<Vec<_>>()
    .join("\n");
  fs::create_dir_all(dir)?;
  fs::write(dir.join(CONFIG_FILE), format!("{header}{}\n", body.trim()))?;
  Ok(true)
}

fn default_config() -> Result<UserConfig, Error> {
  toml::from_str(DEFAULT_CONFIG).map_err(|e| Error::Config(format!("padrão embutido: {e}")))
}

/// O arquivo do padrão embutido vem da comunidade (não precisa ser guardado em `obsidian/`).
/// As configurações de cada plugin (`data.json`) são do usuário e ficam.
fn is_referenced(config: &UserConfig, relative: &Path) -> bool {
  let parts: Vec<String> = relative
    .components()
    .map(|c| c.as_os_str().to_string_lossy().into_owned())
    .collect();
  match parts.as_slice() {
    [kind, id, name] if kind == "plugins" => {
      name != "data.json" && config.plugins.iter().any(|p| &p.id == id)
    }
    [kind, theme, _] if kind == "themes" => config.themes.iter().any(|t| &t.name == theme),
    _ => false,
  }
}

/// Arquivos do plugin na cópia embutida, se ela for da mesma versão.
fn embedded_plugin(plugin: &PluginRef) -> Option<Vec<(String, Vec<u8>)>> {
  let files = embedded_dir(&Path::new("plugins").join(&plugin.id));
  let manifest = files.iter().find(|(n, _)| n == "manifest.json")?;
  let version = serde_json::from_slice::<serde_json::Value>(&manifest.1).ok()?["version"]
    .as_str()?
    .to_string();
  (version == plugin.version).then(|| {
    files
      .into_iter()
      .filter(|(n, _)| n != "data.json")
      .collect()
  })
}

fn embedded_theme(theme: &ThemeRef) -> Option<Vec<(String, Vec<u8>)>> {
  let files = embedded_dir(&Path::new("themes").join(&theme.name));
  (!files.is_empty()).then_some(files)
}

/// Arquivos embutidos diretamente em `dir` (relativo a `.obsidian`), por nome.
fn embedded_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
  crate::obsidian::embedded()
    .into_iter()
    .filter(|(path, _)| path.parent() == Some(dir))
    .filter_map(|(path, bytes)| {
      Some((
        path.file_name()?.to_string_lossy().into_owned(),
        bytes.to_vec(),
      ))
    })
    .collect()
}

/// O que foi registrado por [`save`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveReport {
  pub dir: PathBuf,
  /// Plugins da comunidade, referenciados em `config.toml`.
  pub plugins: Vec<String>,
  /// Plugins fora do catálogo, guardados inteiros em `obsidian/plugins`.
  pub local_plugins: Vec<String>,
  pub themes: Vec<String>,
  pub local_themes: Vec<String>,
  /// Arquivos guardados em `obsidian/`.
  pub files: usize,
}

/// Registra o `.obsidian` de `vault` como padrão do usuário em `dir` (substituindo o anterior).
pub fn save(vault: &Path, dir: &Path) -> Result<SaveReport, Error> {
  let vault = &vault.canonicalize().unwrap_or_else(|_| vault.to_path_buf());
  let source = vault.join(crate::obsidian::CONFIG_DIR);
  if !source.is_dir() {
    return Err(Error::Config(format!(
      "não há .obsidian em {}",
      vault.display()
    )));
  }
  let plugin_catalog = catalog(PLUGIN_CATALOG, "id")?;
  let theme_catalog = catalog(THEME_CATALOG, "name")?;

  let mut config = UserConfig::default();
  let mut report = SaveReport {
    dir: dir.to_path_buf(),
    ..SaveReport::default()
  };
  // (origem, destino relativo a obsidian/)
  let mut keep: Vec<(PathBuf, PathBuf)> = Vec::new();

  for path in walk(&source)? {
    let relative = path.strip_prefix(&source).unwrap_or(&path).to_path_buf();
    let name = relative.file_name().unwrap_or_default().to_string_lossy();
    // Estado da janela e conflitos de sincronização não são configuração.
    if name.starts_with("workspace") && name.ends_with(".json") || name.contains(".sync-conflict-")
    {
      continue;
    }
    let mut parts = relative
      .components()
      .map(|c| c.as_os_str().to_string_lossy().into_owned());
    match (parts.next().as_deref(), parts.next()) {
      (Some("plugins"), Some(id)) if plugin_catalog.contains_key(&id) => {
        // Da comunidade: o código é baixado; só as configurações do plugin ficam guardadas.
        if name == "data.json" {
          keep.push((path.clone(), relative.clone()));
        }
        if name == "manifest.json" {
          let version = manifest_version(&path)?;
          config.plugins.push(PluginRef {
            repo: plugin_catalog[&id].clone(),
            id: id.clone(),
            version,
          });
          report.plugins.push(id);
        }
      }
      (Some("themes"), Some(theme)) if theme_catalog.contains_key(&theme) => {
        if name == "manifest.json" {
          config.themes.push(ThemeRef {
            repo: theme_catalog[&theme].clone(),
            name: theme.clone(),
          });
          report.themes.push(theme);
        }
      }
      (Some("plugins"), Some(id)) => {
        if name == "manifest.json" {
          report.local_plugins.push(id);
        }
        keep.push((path.clone(), relative.clone()));
      }
      (Some("themes"), Some(theme)) => {
        if name == "manifest.json" {
          report.local_themes.push(theme);
        }
        keep.push((path.clone(), relative.clone()));
      }
      _ => keep.push((path.clone(), relative.clone())),
    }
  }

  let files_dir = dir.join(FILES_DIR);
  if files_dir.exists() {
    fs::remove_dir_all(&files_dir)?;
  }
  for (from, relative) in &keep {
    let to = files_dir.join(relative);
    if let Some(parent) = to.parent() {
      fs::create_dir_all(parent)?;
    }
    fs::copy(from, to)?;
  }
  report.files = keep.len();
  for list in [
    &mut report.plugins,
    &mut report.local_plugins,
    &mut report.themes,
    &mut report.local_themes,
  ] {
    list.sort();
  }
  config.plugins.sort_by(|a, b| a.id.cmp(&b.id));
  config.themes.sort_by(|a, b| a.name.cmp(&b.name));

  let header = format!(
    "# Padrão do Obsidian para os livros gerados pelo md-book.\n\
     # Registrado com `md-book --save-config` a partir de {}.\n\
     # Plugins e temas abaixo são baixados da comunidade; o resto está em obsidian/.\n\n",
    vault.display()
  );
  let body = toml::to_string(&config).map_err(|e| Error::Config(e.to_string()))?;
  fs::create_dir_all(dir)?;
  fs::write(dir.join(CONFIG_FILE), header + &body)?;
  Ok(report)
}

fn manifest_version(path: &Path) -> Result<String, Error> {
  let manifest: serde_json::Value = serde_json::from_slice(&fs::read(path)?)
    .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
  manifest["version"]
    .as_str()
    .map(str::to_string)
    .ok_or_else(|| Error::Config(format!("{}: sem campo \"version\"", path.display())))
}

/// Catálogo da comunidade do Obsidian: `key` (id ou nome) → repositório.
fn catalog(url: &str, key: &str) -> Result<HashMap<String, String>, Error> {
  let bytes = fetch(url)?.ok_or_else(|| Error::Network(format!("{url}: não encontrado")))?;
  let entries: Vec<serde_json::Value> =
    serde_json::from_slice(&bytes).map_err(|e| Error::Network(format!("{url}: {e}")))?;
  Ok(
    entries
      .iter()
      .filter_map(|e| {
        Some((
          e[key].as_str()?.to_string(),
          e["repo"].as_str()?.to_string(),
        ))
      })
      .collect(),
  )
}

/// Arquivos do release do plugin, do cache ou baixados (e então guardados no cache).
fn fetch_plugin(plugin: &PluginRef) -> Result<Vec<(String, Vec<u8>)>, Error> {
  let cache = cache_dir().map(|c| c.join("plugins").join(&plugin.id).join(&plugin.version));
  let base = format!(
    "https://github.com/{}/releases/download/{}",
    plugin.repo, plugin.version
  );
  let files = cached(
    cache,
    &["main.js", "manifest.json"],
    &["styles.css"],
    |name| fetch(&format!("{base}/{name}")),
  )
  .map_err(|e| Error::Network(format!("plugin {} {}: {e}", plugin.id, plugin.version)))?;
  Ok(
    files
      .into_iter()
      .map(|(name, bytes)| {
        let bytes = if name == "main.js" {
          as_installed(&bytes)
        } else {
          bytes
        };
        (name, bytes)
      })
      .collect(),
  )
}

/// O `main.js` como o Obsidian o grava ao instalar o plugin: sem o source map embutido e com
/// `/* nosourcemap */` no fim. Assim o arquivo fica idêntico ao de um vault onde o próprio
/// Obsidian instalou o plugin (e o `--apply-config` não acusa diferença).
fn as_installed(main_js: &[u8]) -> Vec<u8> {
  const MAP: &[u8] = b"//# sourceMappingURL=";
  let code = match main_js.windows(MAP.len()).rposition(|w| w == MAP) {
    Some(at) => &main_js[..at],
    None => {
      let end = main_js
        .iter()
        .rposition(|&b| b != b'\n')
        .map_or(0, |i| i + 1);
      &main_js[..end]
    }
  };
  [code, b"\n\n/* nosourcemap */"].concat()
}

fn fetch_theme(theme: &ThemeRef) -> Result<Vec<(String, Vec<u8>)>, Error> {
  let cache = cache_dir().map(|c| c.join("themes").join(&theme.name));
  let base = format!("https://raw.githubusercontent.com/{}/HEAD", theme.repo);
  cached(cache, &["theme.css", "manifest.json"], &[], |name| {
    fetch(&format!("{base}/{name}"))
  })
  .map_err(|e| Error::Network(format!("tema {}: {e}", theme.name)))
}

/// Lê `required` e `optional` do cache; se faltar algum obrigatório, baixa tudo e grava no cache.
fn cached(
  cache: Option<PathBuf>,
  required: &[&str],
  optional: &[&str],
  download: impl Fn(&str) -> Result<Option<Vec<u8>>, Error>,
) -> Result<Vec<(String, Vec<u8>)>, Error> {
  let complete = cache
    .as_ref()
    .is_some_and(|c| c.join(".completo").is_file());
  if let Some(dir) = cache.as_ref().filter(|_| complete) {
    let mut files = Vec::new();
    for name in required.iter().chain(optional) {
      if let Ok(bytes) = fs::read(dir.join(name)) {
        files.push((name.to_string(), bytes));
      }
    }
    return Ok(files);
  }
  let mut files = Vec::new();
  for name in required {
    let bytes = download(name)?.ok_or_else(|| Error::Network(format!("{name} não encontrado")))?;
    files.push((name.to_string(), bytes));
  }
  for name in optional {
    if let Some(bytes) = download(name)? {
      files.push((name.to_string(), bytes));
    }
  }
  if let Some(dir) = cache {
    // O cache é só um atalho: se não der para gravar, segue com o que foi baixado.
    let stored = fs::create_dir_all(&dir).is_ok()
      && files
        .iter()
        .all(|(name, bytes)| fs::write(dir.join(name), bytes).is_ok());
    if stored {
      let _ = fs::write(dir.join(".completo"), "");
    }
  }
  Ok(files)
}

/// Baixa `url`; `None` se não existir (404).
fn fetch(url: &str) -> Result<Option<Vec<u8>>, Error> {
  match ureq::get(url).call() {
    Ok(mut response) => response
      .body_mut()
      .with_config()
      .limit(64 * 1024 * 1024)
      .read_to_vec()
      .map(Some)
      .map_err(|e| Error::Network(format!("{url}: {e}"))),
    Err(ureq::Error::StatusCode(404)) => Ok(None),
    Err(e) => Err(Error::Network(format!("{url}: {e}"))),
  }
}

/// Todos os arquivos sob `dir`, recursivamente.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, Error> {
  let mut out = Vec::new();
  let mut stack = vec![dir.to_path_buf()];
  while let Some(current) = stack.pop() {
    for entry in fs::read_dir(&current)? {
      let path = entry?.path();
      if path.is_dir() {
        stack.push(path);
      } else {
        out.push(path);
      }
    }
  }
  out.sort();
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn config_roundtrips_through_toml() {
    let config = UserConfig {
      plugins: vec![PluginRef {
        id: "vimium-plus".into(),
        repo: "stianlyng/obsidian-vimium-plus".into(),
        version: "0.3.0".into(),
      }],
      themes: vec![ThemeRef {
        name: "Obsidian gruvbox".into(),
        repo: "insanum/obsidian_gruvbox".into(),
      }],
    };
    let text = toml::to_string(&config).unwrap();
    assert!(
      text.contains("[[plugin]]") && text.contains("[[theme]]"),
      "{text}"
    );
    assert_eq!(toml::from_str::<UserConfig>(&text).unwrap(), config);
  }

  #[test]
  fn local_files_need_no_network() {
    let dir = std::env::temp_dir().join(format!("md-book-config-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("obsidian/plugins/colunas-fixas")).unwrap();
    fs::write(dir.join("obsidian/app.json"), "{}").unwrap();
    fs::write(dir.join("obsidian/plugins/colunas-fixas/main.js"), "//").unwrap();
    fs::write(dir.join("config.toml"), "# só arquivos locais\n").unwrap();

    let config = load(&dir).unwrap().expect("config.toml");
    let names: Vec<PathBuf> = user_files(&dir, &config)
      .unwrap()
      .into_iter()
      .map(|(p, _)| p)
      .collect();
    assert_eq!(
      names,
      [
        PathBuf::from("app.json"),
        PathBuf::from("plugins/colunas-fixas/main.js")
      ]
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn default_config_is_created_once_with_only_local_files() {
    let dir = std::env::temp_dir().join(format!("md-book-default-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    assert!(ensure_default(&dir).unwrap());
    assert!(!ensure_default(&dir).unwrap(), "não recria se já existe");

    let config = load(&dir).unwrap().expect("config.toml");
    assert!(config.plugins.iter().any(|p| p.id == "vimium-plus"));
    let stored = walk(&dir.join(FILES_DIR)).unwrap();
    let has = |rel: &str| stored.iter().any(|p| p.ends_with(rel));
    assert!(has("hotkeys.json") && has("plugins/colunas-fixas/main.js"));
    assert!(has("plugins/image-darkmodifier/data.json"));
    assert!(
      !has("plugins/vimium-plus/main.js"),
      "plugins da comunidade não são guardados"
    );

    // O usuário edita o config.toml: a edição é preservada.
    fs::write(dir.join(CONFIG_FILE), "# meu\n").unwrap();
    assert!(!ensure_default(&dir).unwrap());
    assert_eq!(
      fs::read_to_string(dir.join(CONFIG_FILE)).unwrap(),
      "# meu\n"
    );
    fs::remove_dir_all(&dir).unwrap();
  }

  #[test]
  fn embedded_copies_cover_the_default_plugins_offline() {
    let config = default_config().unwrap();
    for plugin in &config.plugins {
      let files =
        embedded_plugin(plugin).unwrap_or_else(|| panic!("{} sem cópia embutida", plugin.id));
      assert!(files.iter().any(|(n, _)| n == "main.js"));
    }
    for theme in &config.themes {
      assert!(embedded_theme(theme).is_some(), "{}", theme.name);
    }
    let other = PluginRef {
      version: "999".into(),
      ..config.plugins[0].clone()
    };
    assert!(
      embedded_plugin(&other).is_none(),
      "versão diferente não usa a cópia embutida"
    );
  }

  #[test]
  fn main_js_matches_what_obsidian_installs() {
    assert_eq!(as_installed(b"code();\n"), b"code();\n\n/* nosourcemap */");
    assert_eq!(
      as_installed(b"code();\n//# sourceMappingURL=data:abc\n"),
      b"code();\n\n\n/* nosourcemap */"
    );
  }

  #[test]
  fn cache_is_used_before_downloading() {
    let dir = std::env::temp_dir().join(format!("md-book-cache-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let download = |name: &str| Ok(Some(format!("baixado {name}").into_bytes()));
    let first = cached(Some(dir.clone()), &["main.js"], &["styles.css"], download).unwrap();
    assert_eq!(first.len(), 2);
    let offline = |_: &str| Err(Error::Network("sem rede".into()));
    let second = cached(Some(dir.clone()), &["main.js"], &["styles.css"], offline).unwrap();
    assert_eq!(second, first);
    fs::remove_dir_all(&dir).unwrap();
  }
}

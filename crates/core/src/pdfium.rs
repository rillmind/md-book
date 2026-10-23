//! Carregamento da biblioteca PDFium.

use std::path::PathBuf;
use std::sync::OnceLock;

use pdfium_render::prelude::*;

use crate::Error;

/// Variável de ambiente com o caminho da libpdfium (arquivo ou diretório).
pub const PDFIUM_ENV: &str = "MD_BOOK_PDFIUM";

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Instância única do PDFium (a biblioteca só pode ser inicializada uma vez por processo).
pub fn pdfium() -> Result<&'static Pdfium, Error> {
  PDFIUM
    .get_or_init(load)
    .as_ref()
    .map_err(|e| Error::PdfiumMissing(e.clone()))
}

#[cfg(feature = "static")]
fn load() -> Result<Pdfium, String> {
  Pdfium::bind_to_statically_linked_library()
    .map(Pdfium::new)
    .map_err(|e| e.to_string())
}

#[cfg(not(feature = "static"))]
fn load() -> Result<Pdfium, String> {
  let mut tried = Vec::new();
  for candidate in candidates() {
    let path = if candidate.is_dir() {
      Pdfium::pdfium_platform_library_name_at_path(&candidate)
    } else {
      candidate
    };
    if !path.is_file() {
      tried.push(path.display().to_string());
      continue;
    }
    return Pdfium::bind_to_library(&path)
      .map(Pdfium::new)
      .map_err(|e| format!("{}: {e}", path.display()));
  }
  Pdfium::bind_to_system_library()
    .map(Pdfium::new)
    .map_err(|_| format!("procurado em: {}", tried.join(", ")))
}

/// Ordem de busca: variável de ambiente, ao lado do executável, `../lib` do executável e,
/// por último, o `vendor/` do repositório (útil em desenvolvimento e nos testes).
#[cfg(not(feature = "static"))]
fn candidates() -> Vec<PathBuf> {
  let mut paths = Vec::new();
  if let Some(env) = std::env::var_os(PDFIUM_ENV) {
    paths.push(PathBuf::from(env));
  }
  if let Some(dir) = std::env::current_exe()
    .ok()
    .and_then(|exe| exe.parent().map(PathBuf::from))
  {
    paths.push(dir.clone());
    paths.push(dir.join("../lib"));
  }
  paths.push(PathBuf::from(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/pdfium/lib"
  )));
  paths
}

//! Escrita do livro como EPUB 3: um XHTML por capítulo, sumário (nav e NCX, para leitores
//! antigos) e as imagens, gerados direto dos blocos, sem passar pelo Markdown.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use zip::CompressionMethod;
use zip::write::{SimpleFileOptions, ZipWriter};

use crate::markdown::IMAGES_DIR;
use crate::model::Style;
use crate::structure::{Block, Chapter, Inline};
use crate::text::ListKind;
use crate::{Book, Error};

/// Idioma quando o livro não informa um (BCP 47: "indeterminado").
const UNKNOWN_LANGUAGE: &str = "und";

const STYLE: &str = "\
body { line-height: 1.5; }
h1, h2, h3, h4, h5, h6 { line-height: 1.2; }
p { margin: 0 0 0.8em; text-align: justify; }
pre { white-space: pre-wrap; font-size: 0.85em; background: #f4f4f4; padding: 0.5em; }
code { font-family: monospace; }
img { max-width: 100%; }
figure { margin: 1em 0; text-align: center; }
table { border-collapse: collapse; margin: 1em 0; }
th, td { border: 1px solid #999; padding: 0.2em 0.5em; vertical-align: top; }
ul.labels { list-style: none; }
aside.notes { margin-top: 2em; border-top: 1px solid #999; font-size: 0.9em; }
";

/// EPUB gravada por [`write_epub`].
#[derive(Debug, Clone)]
pub struct WrittenEpub {
  pub path: PathBuf,
  pub chapters: usize,
  pub images: usize,
}

/// Grava o livro como EPUB em `path`; recusa um arquivo que já existe, a menos que `force`.
///
/// O arquivo é montado ao lado do destino e só substitui o antigo quando está completo.
pub fn write_epub(book: &Book, path: &Path, force: bool) -> Result<WrittenEpub, Error> {
  if path.exists() && !force {
    return Err(Error::OutputExists(path.to_path_buf()));
  }
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir)?;
  }
  // Criado com `File::create` (e não `tempfile`) para ter as permissões normais da umask.
  let mut partial = path.as_os_str().to_owned();
  partial.push(".part");
  let partial = PathBuf::from(partial);
  let images = fs::File::create(&partial)
    .map_err(Error::from)
    .and_then(|mut file| pack(book, &mut file))
    .and_then(|images| Ok(fs::rename(&partial, path).map(|()| images)?));
  if images.is_err() {
    let _ = fs::remove_file(&partial);
  }
  let images = images?;
  Ok(WrittenEpub {
    path: path.to_path_buf(),
    chapters: book.chapters.len(),
    images,
  })
}

fn pack(book: &Book, file: &mut fs::File) -> Result<usize, Error> {
  let zip_err = |e: zip::result::ZipError| Error::Io(std::io::Error::other(e));
  let mut zip = ZipWriter::new(file);
  let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
  let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

  // O `mimetype` tem de ser o primeiro arquivo, sem compressão.
  zip.start_file("mimetype", stored).map_err(zip_err)?;
  zip.write_all(b"application/epub+zip")?;
  zip
    .start_file("META-INF/container.xml", deflated)
    .map_err(zip_err)?;
  zip.write_all(CONTAINER.as_bytes())?;

  let language = book.language.as_deref().unwrap_or(UNKNOWN_LANGUAGE);
  let files: Vec<String> = (1..=book.chapters.len())
    .map(|i| format!("c{i:04}.xhtml"))
    .collect();
  for (chapter, name) in book.chapters.iter().zip(&files) {
    zip
      .start_file(format!("OEBPS/{name}"), deflated)
      .map_err(zip_err)?;
    zip.write_all(render_chapter(chapter, language).as_bytes())?;
  }

  let images = used_images(book);
  if let Some(assets) = &book.assets {
    for image in &images {
      // Imagens já são comprimidas; deflate só gastaria tempo.
      zip
        .start_file(format!("OEBPS/{IMAGES_DIR}/{image}"), stored)
        .map_err(zip_err)?;
      zip.write_all(&fs::read(assets.path().join(image))?)?;
    }
  }

  let toc = toc_entries(book, &files);
  let id = book_id();
  zip
    .start_file("OEBPS/style.css", deflated)
    .map_err(zip_err)?;
  zip.write_all(STYLE.as_bytes())?;
  zip
    .start_file("OEBPS/nav.xhtml", deflated)
    .map_err(zip_err)?;
  zip.write_all(nav(book, &toc, language).as_bytes())?;
  zip.start_file("OEBPS/toc.ncx", deflated).map_err(zip_err)?;
  zip.write_all(ncx(book, &toc, &id).as_bytes())?;
  zip
    .start_file("OEBPS/content.opf", deflated)
    .map_err(zip_err)?;
  zip.write_all(opf(book, &files, &images, &id, language).as_bytes())?;
  zip.finish().map_err(zip_err)?;
  Ok(images.len())
}

const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

/// Imagens usadas pelos capítulos, sem repetir, na ordem em que aparecem.
fn used_images(book: &Book) -> Vec<String> {
  let mut seen = HashSet::new();
  book
    .chapters
    .iter()
    .flat_map(|c| &c.blocks)
    .filter_map(|b| match b {
      Block::Image { file, .. } if seen.insert(file.clone()) => Some(file.clone()),
      _ => None,
    })
    .collect()
}

fn opf(book: &Book, files: &[String], images: &[String], id: &str, language: &str) -> String {
  let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
  writeln!(
    out,
    "<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\" xml:lang=\"{}\">",
    escape(language)
  )
  .unwrap();
  out.push_str("  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n");
  writeln!(out, "    <dc:identifier id=\"bookid\">{id}</dc:identifier>").unwrap();
  writeln!(out, "    <dc:title>{}</dc:title>", escape(&book.title)).unwrap();
  writeln!(out, "    <dc:language>{}</dc:language>", escape(language)).unwrap();
  for author in &book.authors {
    writeln!(out, "    <dc:creator>{}</dc:creator>", escape(author)).unwrap();
  }
  writeln!(
    out,
    "    <meta property=\"dcterms:modified\">{}</meta>",
    utc_timestamp(SystemTime::now())
  )
  .unwrap();
  out.push_str("  </metadata>\n  <manifest>\n");
  out.push_str("    <item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
  out.push_str("    <item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n");
  out.push_str("    <item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n");
  for (i, file) in files.iter().enumerate() {
    writeln!(
      out,
      "    <item id=\"c{}\" href=\"{file}\" media-type=\"application/xhtml+xml\"/>",
      i + 1
    )
    .unwrap();
  }
  for (i, image) in images.iter().enumerate() {
    writeln!(
      out,
      "    <item id=\"img{}\" href=\"{IMAGES_DIR}/{}\" media-type=\"{}\"/>",
      i + 1,
      escape(image),
      media_type(image)
    )
    .unwrap();
  }
  out.push_str("  </manifest>\n  <spine toc=\"ncx\">\n");
  for i in 1..=files.len() {
    writeln!(out, "    <itemref idref=\"c{i}\"/>").unwrap();
  }
  out.push_str("  </spine>\n</package>\n");
  out
}

fn media_type(file: &str) -> &'static str {
  let ext = file.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
  match ext.as_str() {
    "jpg" | "jpeg" => "image/jpeg",
    "gif" => "image/gif",
    "svg" => "image/svg+xml",
    "webp" => "image/webp",
    _ => "image/png",
  }
}

/// Entrada do sumário: profundidade (a partir de 1, sem saltos), título e arquivo.
struct TocEntry<'a> {
  depth: usize,
  title: &'a str,
  file: &'a str,
}

/// Normaliza as profundidades: cada entrada desce no máximo um nível em relação à anterior,
/// como exige a aninhação de listas do nav e do NCX.
fn toc_entries<'a>(book: &'a Book, files: &'a [String]) -> Vec<TocEntry<'a>> {
  let mut prev = 0;
  book
    .chapters
    .iter()
    .zip(files)
    .map(|(c, file)| {
      let depth = (c.depth.max(1) as usize).min(prev + 1);
      prev = depth;
      TocEntry {
        depth,
        title: &c.title,
        file,
      }
    })
    .collect()
}

fn nav(book: &Book, toc: &[TocEntry], language: &str) -> String {
  let mut out = xhtml_head(&book.title, language);
  out.push_str("<nav epub:type=\"toc\" id=\"toc\">\n");
  writeln!(out, "<h1>{}</h1>", escape(&book.title)).unwrap();
  let mut depth = 0;
  for entry in toc {
    if entry.depth > depth {
      out.push_str("<ol>\n");
    } else {
      out.push_str("</li>\n");
      for _ in entry.depth..depth {
        out.push_str("</ol>\n</li>\n");
      }
    }
    depth = entry.depth;
    write!(
      out,
      "<li><a href=\"{}\">{}</a>",
      entry.file,
      escape(entry.title)
    )
    .unwrap();
  }
  for _ in 0..depth {
    out.push_str("</li>\n</ol>\n");
  }
  out.push_str("</nav>\n</body>\n</html>\n");
  out
}

fn ncx(book: &Book, toc: &[TocEntry], id: &str) -> String {
  let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
  out.push_str("<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n");
  writeln!(
    out,
    "<head><meta name=\"dtb:uid\" content=\"{id}\"/></head>\n<docTitle><text>{}</text></docTitle>\n<navMap>",
    escape(&book.title)
  )
  .unwrap();
  let mut depth = 0;
  for (i, entry) in toc.iter().enumerate() {
    // Fecha o irmão anterior e os níveis mais fundos que ele deixou abertos.
    for _ in entry.depth..=depth {
      out.push_str("</navPoint>\n");
    }
    depth = entry.depth;
    writeln!(
      out,
      "<navPoint id=\"p{n}\" playOrder=\"{n}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/>",
      escape(entry.title),
      entry.file,
      n = i + 1
    )
    .unwrap();
  }
  for _ in 0..depth {
    out.push_str("</navPoint>\n");
  }
  out.push_str("</navMap>\n</ncx>\n");
  out
}

fn xhtml_head(title: &str, language: &str) -> String {
  let language = escape(language);
  format!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
     <html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" \
     xml:lang=\"{language}\" lang=\"{language}\">\n<head>\n<meta charset=\"UTF-8\"/>\n\
     <title>{}</title>\n<link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/>\n\
     </head>\n<body>\n",
    escape(title)
  )
}

/// Capítulo em XHTML: título em `h1`, subtítulos relativos à profundidade do capítulo e notas
/// de rodapé no fim.
fn render_chapter(chapter: &Chapter, language: &str) -> String {
  let mut out = xhtml_head(&chapter.title, language);
  writeln!(out, "<h1>{}</h1>", escape(&chapter.title)).unwrap();

  let notes = Notes::new(chapter);
  let mut list = ListWriter::default();
  for block in &chapter.blocks {
    match block {
      Block::ListItem { .. } | Block::Footnote { .. } => {}
      _ => list.close(&mut out),
    }
    match block {
      Block::Heading { level, text } => {
        let h = (level.saturating_sub(chapter.depth) + 1).clamp(2, 6);
        writeln!(out, "<h{h}>{}</h{h}>", escape(text)).unwrap();
      }
      Block::Paragraph(content) => {
        let body = render_inlines(content, &notes);
        if !body.trim().is_empty() {
          writeln!(out, "<p>{body}</p>").unwrap();
        }
      }
      Block::ListItem {
        kind,
        depth,
        content,
      } => {
        let mut body = render_inlines(content, &notes);
        if let ListKind::Label(label) = kind {
          body = format!("{} {body}", escape(label));
        }
        list.item(&mut out, kind, *depth as usize, &body);
      }
      Block::Code(code) => writeln!(out, "<pre><code>{}</code></pre>", escape(code)).unwrap(),
      Block::Image { file, alt } => writeln!(
        out,
        "<figure><img src=\"{IMAGES_DIR}/{}\" alt=\"{}\"/></figure>",
        escape(file),
        escape(alt)
      )
      .unwrap(),
      Block::Table(rows) => render_table(&mut out, rows, &notes),
      Block::Footnote { .. } => {}
    }
  }
  list.close(&mut out);

  let footnotes: Vec<_> = chapter
    .blocks
    .iter()
    .filter_map(|b| match b {
      Block::Footnote { id, content } => Some((id, content)),
      _ => None,
    })
    .collect();
  if !footnotes.is_empty() {
    out.push_str("<aside class=\"notes\">\n");
    let mut written = HashSet::new();
    for (id, content) in footnotes {
      if !written.insert(id) {
        continue;
      }
      let anchor = anchor_id(id);
      let n = notes.numbers[id.as_str()];
      // O link de volta só existe se o texto do capítulo referencia a nota.
      let number = if notes.linked.borrow().contains(id) {
        format!("<a href=\"#ref-{anchor}\">{n}</a>")
      } else {
        n.to_string()
      };
      writeln!(
        out,
        "<aside epub:type=\"footnote\" id=\"fn-{anchor}\"><p>{number}. {}</p></aside>",
        render_inlines(content, &notes)
      )
      .unwrap();
    }
    out.push_str("</aside>\n");
  }
  out.push_str("</body>\n</html>\n");
  out
}

/// Notas de rodapé do capítulo: número de exibição, na ordem em que aparecem, e quais já foram
/// referenciadas no texto.
struct Notes<'a> {
  numbers: HashMap<&'a str, usize>,
  linked: RefCell<HashSet<String>>,
}

impl<'a> Notes<'a> {
  fn new(chapter: &'a Chapter) -> Self {
    let mut numbers = HashMap::new();
    for block in &chapter.blocks {
      if let Block::Footnote { id, .. } = block {
        let next = numbers.len() + 1;
        numbers.entry(id.as_str()).or_insert(next);
      }
    }
    Self {
      numbers,
      linked: RefCell::default(),
    }
  }
}

fn render_table(out: &mut String, rows: &[Vec<Vec<Inline>>], notes: &Notes) {
  let Some((head, body)) = rows.split_first() else {
    return;
  };
  out.push_str("<table>\n<thead><tr>");
  for cell in head {
    write!(out, "<th>{}</th>", render_inlines(cell, notes)).unwrap();
  }
  out.push_str("</tr></thead>\n<tbody>\n");
  for row in body {
    out.push_str("<tr>");
    for cell in row {
      write!(out, "<td>{}</td>", render_inlines(cell, notes)).unwrap();
    }
    out.push_str("</tr>\n");
  }
  out.push_str("</tbody>\n</table>\n");
}

/// Monta listas aninhadas a partir dos itens soltos, cada um com sua profundidade.
#[derive(Default)]
struct ListWriter {
  /// Tags das listas abertas; a do topo tem um `<li>` aberto.
  open: Vec<&'static str>,
}

impl ListWriter {
  fn item(&mut self, out: &mut String, kind: &ListKind, depth: usize, body: &str) {
    let (tag, attrs) = match kind {
      ListKind::Bullet => ("ul", String::new()),
      ListKind::Number(1) => ("ol", String::new()),
      ListKind::Number(n) => ("ol", format!(" start=\"{n}\"")),
      ListKind::Label(_) => ("ul", " class=\"labels\"".to_string()),
    };
    while self.open.len() > depth + 1 {
      out.push_str(&format!("</li></{}>\n", self.open.pop().unwrap()));
    }
    if self.open.len() == depth + 1 {
      out.push_str("</li>\n");
      if self.open.last() != Some(&tag) {
        out.push_str(&format!(
          "</{}>\n<{tag}{attrs}>\n",
          self.open.pop().unwrap()
        ));
        self.open.push(tag);
      }
    } else {
      // Níveis pulados ganham uma lista intermediária, para o aninhamento continuar válido.
      while self.open.len() < depth {
        out.push_str("<ul class=\"labels\"><li>");
        self.open.push("ul");
      }
      out.push_str(&format!("<{tag}{attrs}>\n"));
      self.open.push(tag);
    }
    out.push_str(&format!("<li>{body}"));
  }

  fn close(&mut self, out: &mut String) {
    while let Some(tag) = self.open.pop() {
      out.push_str(&format!("</li></{tag}>\n"));
    }
  }
}

/// Trechos com estilo em XHTML, juntando os vizinhos de mesmo estilo.
fn render_inlines(content: &[Inline], notes: &Notes) -> String {
  let mut runs: Vec<(String, Style)> = Vec::new();
  let mut out = String::new();
  let flush = |runs: &mut Vec<(String, Style)>, out: &mut String| {
    for (text, style) in runs.drain(..) {
      out.push_str(&styled(&text, style));
    }
  };
  for inline in content {
    match inline {
      Inline::Text { text, style } => {
        let text: String = text.chars().filter(|&c| c != '\u{ad}').collect();
        match runs.last_mut() {
          Some((prev, prev_style)) if prev_style == style => prev.push_str(&text),
          _ => runs.push((text, *style)),
        }
      }
      Inline::Break => {
        flush(&mut runs, &mut out);
        out.push_str("<br/>");
      }
      Inline::Sup(t) => {
        flush(&mut runs, &mut out);
        write!(out, "<sup>{}</sup>", escape(t)).unwrap();
      }
      Inline::Sub(t) => {
        flush(&mut runs, &mut out);
        write!(out, "<sub>{}</sub>", escape(t)).unwrap();
      }
      Inline::FootnoteRef(id) => {
        flush(&mut runs, &mut out);
        match notes.numbers.get(id.as_str()) {
          Some(n) => {
            let anchor = anchor_id(id);
            // Só a primeira referência leva o id que o link de volta da nota usa.
            let target = if notes.linked.borrow_mut().insert(id.clone()) {
              format!(" id=\"ref-{anchor}\"")
            } else {
              String::new()
            };
            write!(
              out,
              "<sup><a epub:type=\"noteref\"{target} href=\"#fn-{anchor}\">{n}</a></sup>"
            )
            .unwrap();
          }
          None => write!(out, "<sup>{}</sup>", escape(id)).unwrap(),
        }
      }
    }
  }
  flush(&mut runs, &mut out);
  out
}

/// Envolve em `strong`/`em`/`code`, deixando os espaços das pontas do lado de fora.
fn styled(text: &str, style: Style) -> String {
  let core = text.trim();
  if core.is_empty() || style == Style::default() {
    return escape(text);
  }
  let lead = &text[..text.len() - text.trim_start().len()];
  let trail = &text[text.trim_end().len()..];
  let mut inner = escape(core);
  if style.mono {
    inner = format!("<code>{inner}</code>");
  }
  if style.italic {
    inner = format!("<em>{inner}</em>");
  }
  if style.bold {
    inner = format!("<strong>{inner}</strong>");
  }
  format!("{}{inner}{}", escape(lead), escape(trail))
}

/// Escapa para XML e descarta os caracteres de controle que o XML não aceita.
fn escape(text: &str) -> String {
  let mut out = String::with_capacity(text.len());
  for c in text.chars() {
    match c {
      '&' => out.push_str("&amp;"),
      '<' => out.push_str("&lt;"),
      '>' => out.push_str("&gt;"),
      '"' => out.push_str("&quot;"),
      '\t' | '\n' | '\r' => out.push(c),
      c if (c as u32) < 0x20 || c == '\u{fffe}' || c == '\u{ffff}' => {}
      c => out.push(c),
    }
  }
  out
}

/// Id de nota utilizável como âncora XML.
fn anchor_id(id: &str) -> String {
  id.chars()
    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
    .collect()
}

/// Identificador único da edição (UUID v4 aleatório).
fn book_id() -> String {
  use std::hash::{BuildHasher, Hasher};
  let random = || {
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
      SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos()),
    );
    h.finish()
  };
  let mut bytes = [random().to_be_bytes(), random().to_be_bytes()].concat();
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
  format!(
    "urn:uuid:{}-{}-{}-{}-{}",
    &hex[..8],
    &hex[8..12],
    &hex[12..16],
    &hex[16..20],
    &hex[20..]
  )
}

/// `AAAA-MM-DDThh:mm:ssZ`, o formato exigido em `dcterms:modified`.
fn utc_timestamp(time: SystemTime) -> String {
  let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
  let (days, rest) = (secs / 86_400, secs % 86_400);
  // Data civil a partir dos dias desde 1970-01-01 (algoritmo de Howard Hinnant).
  let z = days as i64 + 719_468;
  let era = z.div_euclid(146_097);
  let doe = z - era * 146_097;
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let day = doy - (153 * mp + 2) / 5 + 1;
  let month = if mp < 10 { mp + 3 } else { mp - 9 };
  let year = yoe + era * 400 + i64::from(month <= 2);
  format!(
    "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
    rest / 3600,
    rest % 3600 / 60,
    rest % 60
  )
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::*;

  fn item(kind: ListKind, depth: u8, text: &str) -> Block {
    Block::ListItem {
      kind,
      depth,
      content: vec![Inline::plain(text)],
    }
  }

  fn body(chapter: &Chapter) -> String {
    let html = render_chapter(chapter, "pt-BR");
    let start = html.find("<body>\n").unwrap() + 7;
    let end = html.find("</body>").unwrap();
    html[start..end].to_string()
  }

  #[test]
  fn timestamps_are_utc_in_epub_format() {
    assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    let t = UNIX_EPOCH + Duration::from_secs(1_793_356_245);
    assert_eq!(utc_timestamp(t), "2026-10-30T10:30:45Z");
  }

  #[test]
  fn book_ids_are_uuid_urns() {
    let id = book_id();
    assert_eq!(id.len(), "urn:uuid:".len() + 36);
    assert_eq!(&id[23..24], "4");
    assert_ne!(id, book_id());
  }

  #[test]
  fn escapes_markup_and_drops_invalid_xml_characters() {
    assert_eq!(escape("a < b & \"c\"\u{1}"), "a &lt; b &amp; &quot;c&quot;");
  }

  #[test]
  fn nests_lists_by_depth() {
    let c = Chapter {
      title: "Cap".into(),
      depth: 1,
      blocks: vec![
        item(ListKind::Number(1), 0, "um"),
        item(ListKind::Bullet, 1, "sub"),
        item(ListKind::Number(2), 0, "dois"),
        Block::Paragraph(vec![Inline::plain("fim")]),
      ],
    };
    assert_eq!(
      body(&c),
      "<h1>Cap</h1>\n<ol>\n<li>um<ul>\n<li>sub</li></ul>\n</li>\n<li>dois</li></ol>\n<p>fim</p>\n"
    );
  }

  #[test]
  fn renders_styles_footnotes_and_subheadings() {
    let bold = Style {
      bold: true,
      ..Style::default()
    };
    let c = Chapter {
      title: "Seção".into(),
      depth: 2,
      blocks: vec![
        Block::Heading {
          level: 3,
          text: "Sub".into(),
        },
        Block::Paragraph(vec![
          Inline::plain("um "),
          Inline::Text {
            text: "forte ".into(),
            style: bold,
          },
          Inline::plain("x"),
          Inline::FootnoteRef("3-1".into()),
        ]),
        Block::Footnote {
          id: "3-1".into(),
          content: vec![Inline::plain("A nota.")],
        },
      ],
    };
    assert_eq!(
      body(&c),
      "<h1>Seção</h1>\n<h2>Sub</h2>\n\
       <p>um <strong>forte</strong> x<sup><a epub:type=\"noteref\" id=\"ref-3-1\" href=\"#fn-3-1\">1</a></sup></p>\n\
       <aside class=\"notes\">\n\
       <aside epub:type=\"footnote\" id=\"fn-3-1\"><p><a href=\"#ref-3-1\">1</a>. A nota.</p></aside>\n\
       </aside>\n"
    );
  }

  #[test]
  fn repeated_and_unreferenced_notes_keep_anchors_valid() {
    let note = |id: &str| Block::Footnote {
      id: id.into(),
      content: vec![Inline::plain("nota")],
    };
    let c = Chapter {
      title: "Cap".into(),
      depth: 1,
      blocks: vec![
        Block::Paragraph(vec![
          Inline::FootnoteRef("a".into()),
          Inline::FootnoteRef("a".into()),
        ]),
        note("a"),
        note("b"),
        note("a"),
      ],
    };
    let html = body(&c);
    assert_eq!(html.matches("id=\"ref-a\"").count(), 1, "{html}");
    assert_eq!(html.matches("id=\"fn-a\"").count(), 1, "{html}");
    assert!(html.contains("id=\"fn-b\"><p>2. nota"), "{html}");
    assert!(!html.contains("#ref-b"), "{html}");
  }

  #[test]
  fn toc_never_skips_levels() {
    let book = Book {
      title: "Livro".into(),
      authors: vec![],
      language: None,
      chapters: [1, 3, 2, 1]
        .into_iter()
        .map(|depth| Chapter {
          title: format!("d{depth}"),
          depth,
          blocks: vec![],
        })
        .collect(),
      heading_source: crate::HeadingSource::None,
      outline_ignored: false,
      failed_pages: vec![],
      assets: None,
    };
    let files: Vec<String> = (1..=4).map(|i| format!("c{i}.xhtml")).collect();
    let toc = toc_entries(&book, &files);
    let depths: Vec<usize> = toc.iter().map(|e| e.depth).collect();
    assert_eq!(depths, [1, 2, 2, 1]);
    let nav = nav(&book, &toc, "und");
    assert_eq!(nav.matches("<ol>").count(), nav.matches("</ol>").count());
    assert_eq!(nav.matches("<li>").count(), nav.matches("</li>").count());
    let ncx = ncx(&book, &toc, "x");
    assert_eq!(
      ncx.matches("<navPoint ").count(),
      ncx.matches("</navPoint>").count()
    );
  }
}

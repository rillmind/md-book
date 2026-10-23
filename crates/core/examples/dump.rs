//! Mostra as linhas extraídas de uma página: `cargo run -p mdbook_gen_core --example dump -- livro.pdf 12`

fn main() {
  let args: Vec<String> = std::env::args().collect();
  let (Some(path), Some(page)) = (args.get(1), args.get(2).and_then(|p| p.parse().ok())) else {
    eprintln!("uso: dump <arquivo.pdf> <página>");
    std::process::exit(2);
  };
  match mdbook_gen_core::debug_page(std::path::Path::new(path), page) {
    Ok(text) => print!("{text}"),
    Err(e) => {
      eprintln!("{e}");
      std::process::exit(1);
    }
  }
}

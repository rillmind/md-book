# Plano: fidelidade da conversão (próxima feature)

> Registrado em 2026-10-21. Status: **planejado, não iniciado**.

## Contexto

A v0.1 converte bem PDFs de texto simples, mas tem cinco limitações que o usuário considera
**fundamentais** para a aplicação:

1. PDFs em **duas ou mais colunas** saem com linhas misturadas.
2. **Tabelas** viram texto corrido.
3. **Negrito, itálico e código (monoespaçado)** se perdem.
4. **Imagens** são descartadas.
5. PDFs **escaneados** falham (sem OCR).

A causa comum de 1–4 é o motor de extração: o `pdf-extract` entrega só caractere + tamanho de
fonte + posição. Não informa nome/peso da fonte, não expõe imagens nem os traços (linhas de
tabela) da página. Por isso o primeiro passo é trocar o motor.

## Decisão de base: migrar a extração para `pdfium-render`

`pdfium-render` (wrapper do PDFium, o motor de PDF do Chrome; licença Apache/BSD) fornece:

- por caractere: `font_name()`, `font_weight()`, `font_is_italic()`, `scaled_font_size()`,
  `loose_bounds()` → base para estilos e análise de layout;
- objetos da página: imagens (`get_raw_image()`) e caminhos (linhas/retângulos → grades de tabela);
- bookmarks (substitui o `lopdf::get_toc`) e renderização de página em bitmap (entrada do OCR).

Custo: precisa da biblioteca nativa `libpdfium`. Opções (decidir antes de começar):
- **dinâmica** — distribuir `libpdfium.so/.dll/.dylib` ao lado do binário (binários prontos em
  `bblanchon/pdfium-binaries`); build simples.
- **estática** — feature `static` do `pdfium-render`; binário único, build mais trabalhoso.

## Novo modelo intermediário (`crates/core/src/model.rs`)

```rust
struct Page { number, width, height, spans: Vec<Span>, images: Vec<ImageObj>, rules: Vec<Segment> }
struct Span { text, bbox: Rect, font_size, bold, italic, mono }
struct ImageObj { bbox: Rect, png: Vec<u8> /* ou caminho temporário */ }
struct Segment { from: (f32, f32), to: (f32, f32) }   // linhas horizontais/verticais
```

`Block` ganha `Table`, `Image`, `Code` e parágrafos passam a ter runs com estilo:
`Paragraph(Vec<Inline>)`, `Inline { text, bold, italic, code }`.

## Fases (cada uma = um commit `feat`, com testes)

### F1 — Motor pdfium + modelo rico (sem mudança de comportamento)
- `extract.rs` reescrito sobre `pdfium-render`, produzindo `Page` com spans.
- Agrupamento span → linha reaproveita a lógica atual (y + tolerância).
- Outline via pdfium. Todos os testes atuais devem continuar passando.
- Remover `pdf-extract`.

### F2 — Layout multi-coluna (ordem de leitura)
- Por página, **XY-cut recursivo**: corta onde há faixas verticais/horizontais vazias
  maiores que um limiar (relativo ao tamanho do corpo) → blocos de coluna.
- Ordem: de cima para baixo; dentro de uma faixa, colunas da esquerda para a direita.
- Títulos que atravessam as colunas (largura total) ficam no topo da faixa.
- Fixture: seção de 2 colunas no `gerar.py` (LibreOffice "Formatar → Colunas").

### F3 — Negrito, itálico e código
- Estilo do span: negrito se `font_weight >= 600` ou nome contém Bold/Black/Heavy/Semibold;
  itálico se flag ou nome contém Italic/Oblique; mono se nome contém
  Courier/Mono/Consolas/Menlo/Code.
- Runs adjacentes de mesmo estilo são unidos; espaços ficam **fora** dos marcadores
  (`**palavra** seguinte`, nunca `**palavra **`).
- Linhas consecutivas inteiramente mono → bloco de código cercado (```), preservando indentação.
- Estilo uniforme num título inteiro é ignorado (o título já é destaque).

### F4 — Imagens
- Extrai imagens raster para `src/images/cNN-iK.png` e insere `![](images/...)` na posição de
  leitura (pelo bbox).
- Ignora imagens minúsculas (< 32 px) e repetidas em muitas páginas (hash → logos/marcas d'água).
- Flag `--no-images`. Gravação em streaming (não acumular imagens na memória).
- Fora de escopo inicial: figuras vetoriais (possível depois: renderizar a região do bbox).

### F5 — Tabelas
- **Com bordas**: segmentos horizontais + verticais formando grade → células pelas interseções.
- **Sem bordas**: ≥ 3 linhas seguidas com ≥ 2 colunas alinhadas nos mesmos x.
- Saída em tabela GFM (`| a | b |`, o mdBook suporta); `|` escapado; quebras de linha na
  célula → `<br>`. Células mescladas/irregulares → mantém como texto (sem inventar estrutura).
- Fixtures: uma tabela com bordas e uma sem.

### F6 — OCR (feature cargo `ocr`)
- Página sem texto (ou quase) → renderiza com pdfium a 300 dpi → Tesseract.
- Usar saída TSV/hOCR (palavras com bbox) para gerar `Span`s e reaproveitar todo o pipeline;
  tamanho de fonte estimado pela altura das caixas.
- Flags `--ocr auto|always|never` (padrão `auto`) e `--ocr-lang por+eng`.
- Requer Tesseract + idioma: `sudo dnf install tesseract-langpack-por`
  (hoje a máquina só tem `eng`).

## Decisões em aberto (perguntar antes de iniciar)

1. pdfium **dinâmico ou estático**? (recomendação: dinâmico primeiro, estático depois)
2. OCR com **Tesseract** (melhor para português, dependência de sistema) ou **`ocrs`**
   (Rust puro, binário único, modelos focados em inglês)? Recomendação: Tesseract.
3. Imagens: só raster, ou também renderizar figuras vetoriais?

## Verificação

- Cada fase estende `crates/core/tests/fixtures/gerar.py` e `tests/fixtures.rs`.
- Testes de "golden file": Markdown esperado versionado ao lado de cada fixture.
- Rodar `mdbook build` sobre a saída (via `--build`) em todas as fixtures.
- Repetir a medição de desempenho (PDF de 360 páginas: hoje 0,68 s / 24 MB de pico) e garantir
  que imagens e OCR não estourem memória (`/usr/bin/time -f "%e s %M KB"`).

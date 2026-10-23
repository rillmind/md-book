# Plano: fidelidade da conversão (próxima feature)

> Registrado em 2026-10-21. Status (2026-10-23): **F1–F6 implementadas** na branch `dev`;
> F7 (OCR) e o PDFium estático pendentes.

## Situação

| Fase | Status | Commit |
| --- | --- | --- |
| F1 Motor pdfium | feito | `refactor(core): troca pdf-extract pelo pdfium na extração` |
| F2 Qualidade do texto | feito | `feat(core): melhora a qualidade do texto` |
| F3 Várias colunas | feito | `feat(core): lê páginas em várias colunas na ordem certa` |
| F4 Negrito, itálico, código, notas | feito | `feat(core): preserva negrito, itálico, código e notas de rodapé` |
| F5 Imagens e figuras vetoriais | feito | `feat(core): extrai imagens e figuras para src/images` |
| F6 Tabelas | feito | `feat(core): converte tabelas em tabelas markdown` |
| F7 OCR | pendente | — |

Corpus de regressão (links em `corpus/`, fora do git): Tanenbaum, Kurose, How Linux Works,
Competitive Programmer's Handbook, Édipo Rei, apostilas e slides. Todos geram o livro e o HTML
do mdBook; o maior (778 páginas) leva ~7 s e ~280 MB de pico.

Limitações conhecidas: tabelas sem nenhum filete continuam texto; diagramas de caixas alinhadas
às vezes viram tabela; texto com espaçamento entre letras ("NOT E") vem assim do PDFium.

## Contexto

A v0.1 converte bem PDFs de texto simples, mas tem cinco limitações que o usuário considera
**fundamentais** para a aplicação:

1. PDFs em **duas ou mais colunas** saem com linhas misturadas.
2. **Tabelas** viram texto corrido.
3. **Negrito, itálico e código (monoespaçado)** se perdem.
4. **Imagens** são descartadas.
5. PDFs **escaneados** falham (sem OCR).

Feedback do usuário após testar a v0.1 com PDFs reais: **o texto corrido funcionou, mas o
resto sai muitas vezes mal formado** (“péssimo”). Por isso a qualidade do texto virou uma fase
própria (F2), prioritária, antes das features novas.

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

### F2 — Qualidade do texto (prioridade, antes das features novas)
Primeiro, montar um **corpus de regressão** com os PDFs reais que o usuário testou:
`corpus/` fora do git (direitos autorais), com a saída Markdown revisada à mão como gabarito
e um teste que compara e mostra o diff. Os problemas conhecidos na v0.1, e o que fazer:
- **Escape excessivo**: `\_`, `\*`, `\[` em todo lugar deixam o `.md` ilegível. Escapar só
  quando o caractere de fato formaria sintaxe (ex.: `*` envolvendo uma palavra, `<` seguido de
  letra, `[`…`](`), e manter o fonte limpo.
- **Hifenização**: hoje "bem-" + "estar" vira "bemestar". Só remover o hífen se a palavra
  unida existir na própria obra (contagem de palavras do livro) ou se a parte antes do hífen
  não for uma palavra sozinha; senão manter "bem-estar". Tratar o soft hyphen (U+00AD).
- **Parágrafos**: usar o **recuo da primeira linha** (padrão de livros) além do espaço
  vertical; detectar a margem esquerda do corpo por página.
- **Títulos**: além do tamanho, usar negrito/caixa alta/linha isolada com espaço acima e abaixo
  (depende do F1); numeração no início ("1.2", "Capítulo 3", "CAPÍTULO III").
- **Listas**: numeradas (`1.`, `1)`, `a)`, `i.`) e continuação de item pelo recuo.
- **Notas de rodapé**: bloco de fonte menor no pé da página → nota mdBook (`[^1]`) ou
  removida; marcador sobrescrito no texto vira referência.
- **Normalização Unicode**: ligaduras (ﬁ, ﬂ), espaços estranhos e aspas → NFKC seletivo.
- **Espaços entre palavras**: com o pdfium, usar o espaçamento real das letras (e os
  caracteres de espaço gerados) em vez do limiar fixo, para não colar nem partir palavras.
- **Sobrescrito/subscrito e capitulares**: não podem quebrar a linha nem virar título.
- **Linhas de página**: cabeçalhos e rodapés que variam (título do capítulo em páginas
  pares/ímpares) devem ser reconhecidos pela posição, mesmo sem repetir o texto exato.

Critério de pronto: o corpus inteiro gera Markdown legível no próprio `.md`, sem regressão
no texto corrido.

### F3 — Layout multi-coluna (ordem de leitura)
- Por página, **XY-cut recursivo**: corta onde há faixas verticais/horizontais vazias
  maiores que um limiar (relativo ao tamanho do corpo) → blocos de coluna.
- Ordem: de cima para baixo; dentro de uma faixa, colunas da esquerda para a direita.
- Títulos que atravessam as colunas (largura total) ficam no topo da faixa.
- Fixture: seção de 2 colunas no `gerar.py` (LibreOffice "Formatar → Colunas").

### F4 — Negrito, itálico e código
- Estilo do span: negrito se `font_weight >= 600` ou nome contém Bold/Black/Heavy/Semibold;
  itálico se flag ou nome contém Italic/Oblique; mono se nome contém
  Courier/Mono/Consolas/Menlo/Code.
- Runs adjacentes de mesmo estilo são unidos; espaços ficam **fora** dos marcadores
  (`**palavra** seguinte`, nunca `**palavra **`).
- Linhas consecutivas inteiramente mono → bloco de código cercado (```), preservando indentação.
- Estilo uniforme num título inteiro é ignorado (o título já é destaque).

### F5 — Imagens
- Extrai imagens raster para `src/images/cNN-iK.png` e insere `![](images/...)` na posição de
  leitura (pelo bbox).
- Ignora imagens minúsculas (< 32 px) e repetidas em muitas páginas (hash → logos/marcas d'água).
- Flag `--no-images`. Gravação em streaming (não acumular imagens na memória).
- Fora de escopo inicial: figuras vetoriais (possível depois: renderizar a região do bbox).

### F6 — Tabelas
- **Com bordas**: segmentos horizontais + verticais formando grade → células pelas interseções.
- **Sem bordas**: ≥ 3 linhas seguidas com ≥ 2 colunas alinhadas nos mesmos x.
- Saída em tabela GFM (`| a | b |`, o mdBook suporta); `|` escapado; quebras de linha na
  célula → `<br>`. Células mescladas/irregulares → mantém como texto (sem inventar estrutura).
- Fixtures: uma tabela com bordas e uma sem.

### F7 — OCR (feature cargo `ocr`)
- Página sem texto (ou quase) → renderiza com pdfium a 300 dpi → Tesseract.
- Usar saída TSV/hOCR (palavras com bbox) para gerar `Span`s e reaproveitar todo o pipeline;
  tamanho de fonte estimado pela altura das caixas.
- Flags `--ocr auto|always|never` (padrão `auto`) e `--ocr-lang por+eng`.
- Requer Tesseract + idioma: `sudo dnf install tesseract-langpack-por`
  (hoje a máquina só tem `eng`).

## Decisões

1. PDFium: o usuário escolheu **estático**. Não há `libpdfium.a` pré-compilada para Linux
   (bblanchon e paulocoutinhox só publicam a `.so`); falta decidir entre compilar o PDFium do
   código-fonte (depot_tools/gn, vários GB, ~1 h) ou embutir a `.so` no binário e extraí-la na
   primeira execução. Até lá: `.so` em `vendor/` (feature `static` já existe no core).
2. OCR com **Tesseract** (melhor para português, dependência de sistema) ou **`ocrs`**
   (Rust puro, binário único, modelos focados em inglês)? Recomendação: Tesseract. Pendente.
3. Imagens: raster **e** figuras vetoriais (feito).

## Verificação

- Cada fase estende `crates/core/tests/fixtures/gerar.py` e `tests/fixtures.rs`.
- Testes de "golden file": Markdown esperado versionado ao lado de cada fixture.
- Rodar `mdbook build` sobre a saída (via `--build`) em todas as fixtures.
- Repetir a medição de desempenho (PDF de 360 páginas: hoje 0,68 s / 24 MB de pico) e garantir
  que imagens e OCR não estourem memória (`/usr/bin/time -f "%e s %M KB"`).

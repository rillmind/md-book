"""Gera os PDFs de teste. Requer LibreOffice. Uso: python3 gerar.py (dentro desta pasta)."""
import random
import subprocess

random.seed(42)
WORDS = ("documento página texto formato conversão capítulo leitura estrutura arquivo detalhe "
         "processo informação conteúdo sistema exemplo resultado método análise projeto livro "
         "parte seção ideia tempo forma modelo dado ferramenta código versão").split()


def sentence():
    words = random.choices(WORDS, k=random.randint(8, 18))
    return " ".join(words).capitalize() + "."


def para(n):
    return "<p>" + " ".join(sentence() for _ in range(n)) + "</p>"


html = ['<html><head><meta charset="utf-8"><title>Guia de Teste</title></head><body>',
        '<p style="font-size:40pt;text-align:center">Guia de Teste</p>',
        '<p style="page-break-after:always">Uma capa simples.</p>']
for c, name in enumerate(["Introdução", "Instalação e Configuração", "Uso Avançado"], 1):
    html += [f'<h1 style="page-break-before:always">{c} {name}</h1>', para(6), para(4),
             f"<h2>{c}.1 Primeiros passos</h2>", para(5),
             "<ul><li>Primeiro item da lista</li><li>Segundo item da lista</li></ul>",
             f"<h2>{c}.2 Detalhes</h2>", para(12), para(10)]
html.append("</body></html>")
open("livro.html", "w").write("\n".join(html))


def lo(*args):
    subprocess.run(["libreoffice", "--headless", *args], check=True, capture_output=True)


# Via ODT: o Writer exporta todos os títulos como bookmarks.
lo("--infilter=HTML (StarWriter)", "--convert-to", "odt", "livro.html")
lo("--convert-to", "pdf", "livro.odt")
subprocess.run(["mv", "livro.pdf", "outline-completo.pdf"], check=True)
# Direto do HTML (Writer/Web): só os h2 viram bookmarks — outline incompleto.
lo("--convert-to", "pdf:writer_web_pdf_Export", "livro.html")
subprocess.run(["mv", "livro.pdf", "outline-incompleto.pdf"], check=True)
lo("--convert-to",
   'pdf:writer_web_pdf_Export:{"ExportBookmarks":{"type":"boolean","value":"false"}}',
   "livro.html")
subprocess.run(["mv", "livro.pdf", "sem-outline.pdf"], check=True)
subprocess.run(["rm", "livro.html", "livro.odt"], check=True)


def png(path, w, h):
    """PNG em degradê, sem depender de bibliotecas de imagem."""
    import struct
    import zlib
    rows = b"".join(
        b"\x00" + b"".join(bytes((x * 255 // w, y * 255 // h, 128)) for x in range(w)) for y in range(h)
    )

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


# Figuras: uma foto (raster) e um diagrama vetorial com rótulos.
png("foto.png", 320, 200)
open("diagrama.svg", "w").write("""<svg xmlns="http://www.w3.org/2000/svg" width="400" height="160">
<rect x="10" y="40" width="120" height="60" fill="none" stroke="black" stroke-width="2"/>
<text x="35" y="75" font-size="14">Cliente</text>
<line x1="130" y1="70" x2="270" y2="70" stroke="black" stroke-width="2"/>
<polygon points="270,64 282,70 270,76" fill="black"/>
<rect x="282" y="40" width="110" height="60" fill="none" stroke="black" stroke-width="2"/>
<text x="302" y="75" font-size="14">Servidor</text>
</svg>""")
html = ['<html><head><meta charset="utf-8"><title>Figuras</title></head><body>',
        '<h1>1 Figuras</h1>', para(4),
        '<p><img src="foto.png" width="320" height="200"></p>',
        '<p>Figura 1.1 Uma foto em degradê.</p>', para(3),
        '<p><img src="diagrama.svg" width="400" height="160"></p>',
        '<p>Figura 1.2 Cliente e servidor.</p>', para(3),
        '</body></html>']
open("figuras.html", "w").write("\n".join(html))
lo("--infilter=HTML (StarWriter)", "--convert-to", "odt", "figuras.html")
lo("--convert-to", "pdf", "figuras.odt")
subprocess.run(["rm", "figuras.html", "figuras.odt", "foto.png", "diagrama.svg"], check=True)


# Tabela com bordas, cabeçalho e uma célula que quebra em duas linhas.
rows = [("Comando", "Função"), ("ls", "Lista arquivos"), ("cp", "Copia arquivos e diretórios entre dois lugares quaisquer do sistema"),
        ("rm", "Remove arquivos")]
cells = "".join(f"<tr>{''.join(f'<{t}>{c}</{t}>' for c in r)}</tr>" for r, t in zip(rows, ["th", "td", "td", "td"]))
html = ['<html><head><meta charset="utf-8"><title>Tabelas</title></head><body>', "<h1>1 Tabelas</h1>", para(3),
        "<p>Tabela 1.1 Comandos básicos.</p>", f'<table border="1" cellpadding="4" width="60%">{cells}</table>', para(3),
        "</body></html>"]
open("tabela.html", "w").write("\n".join(html))
lo("--infilter=HTML (StarWriter)", "--convert-to", "odt", "tabela.html")
lo("--convert-to", "pdf", "tabela.odt")
subprocess.run(["rm", "tabela.html", "tabela.odt"], check=True)

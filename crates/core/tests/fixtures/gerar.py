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

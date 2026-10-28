const { Plugin, MarkdownView } = require("obsidian");

// O modo de leitura só desenha os trechos perto da tela e vai trocando-os ao
// rolar; com colunas, cada troca reorganiza a página inteira. Ampliando a
// margem de pré-renderização do renderizador (API interna), a nota é desenhada
// toda de uma vez e o layout fica fixo.
const TUDO = 1e9;
const PADRAO = { renderExtra: 1, renderExtraMinPx: 500 };

module.exports = class ColunasFixas extends Plugin {
  onload() {
    this.app.workspace.onLayoutReady(() => this.aplicar(TUDO, TUDO));
    this.registerEvent(this.app.workspace.on("layout-change", () => this.aplicar(TUDO, TUDO)));
  }

  onunload() {
    this.aplicar(PADRAO.renderExtra, PADRAO.renderExtraMinPx);
  }

  aplicar(extra, extraMinPx) {
    for (const leaf of this.app.workspace.getLeavesOfType("markdown")) {
      if (!(leaf.view instanceof MarkdownView)) continue;
      const renderer = leaf.view.previewMode && leaf.view.previewMode.renderer;
      if (!renderer || renderer.renderExtraMinPx === extraMinPx) continue;
      renderer.renderExtra = extra;
      renderer.renderExtraMinPx = extraMinPx;
      if (typeof renderer.updateVirtualDisplay === "function") renderer.updateVirtualDisplay();
    }
  }
};

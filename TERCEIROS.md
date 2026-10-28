# Componentes de terceiros

O `md-book` embute, em `crates/core/obsidian/`, as configurações padrão do Obsidian que copia para
cada livro gerado. Elas incluem plugins e temas de terceiros, redistribuídos sem modificação sob
as licenças dos autores:

| Componente | Tipo | Autor | Repositório | Licença |
| --- | --- | --- | --- | --- |
| obsidian-hyphenation | plugin | Cassandra McCarthy | https://github.com/7596ff/obsidian-hyphenation | MIT |
| image-darkmodifier | plugin | 0qln | https://github.com/0qln/obsidian-image-darkmodifier | MIT |
| vimakase | plugin | Vilem Jelen | https://github.com/jelenv/obsidian-vimakase | MIT |
| vimium-plus | plugin | Stian Lyng | https://github.com/stianlyng/obsidian-vimium-plus | MIT |
| Obsidian gruvbox | tema | insanum | https://github.com/insanum/obsidian_gruvbox | MIT |
| Material Gruvbox | tema | alljavi | https://github.com/alljavi/material_gruvbox_obsidian | MIT |

O plugin `colunas-fixas` e os snippets CSS são deste projeto.

Os textos completos das licenças estão nos repositórios acima. Para atualizar o snapshot a partir
de um vault: `scripts/atualizar-obsidian.sh [vault]`, e depois recompile.

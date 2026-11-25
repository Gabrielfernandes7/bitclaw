# Markdown da Microsoft Learn

Leia este arquivo só ao ler ou escrever docs da Microsoft Learn (repositórios com `docfx.json`, frontmatter com `ms.author`/`ms.topic`). Base: CommonMark via Markdig, mais extensões próprias.

## Regras de estilo

- Exatamente um H1, logo depois do frontmatter. Os H2 aparecem no menu lateral, então use-os com cuidado.
- Evite HTML cru: gera avisos ou erros de build.
- Evite alertas. Se usar, no máximo um ou dois por artigo, nunca lado a lado.
- Numeradas aninhadas com números (renderizam como letras). Não use letras.
- Use `-` ou `*` de forma consistente.

## Alertas

```md
> [!NOTE]
> Informação que o leitor deve notar mesmo lendo por cima.
```

Tipos: `NOTE`, `TIP`, `IMPORTANT`, `CAUTION`, `WARNING`.

## Imagens

Padrão: `![alt](caminho.png)`. Formato recomendado:

```md
:::image type="content" source="media/arq/img.png" alt-text="Descrição":::
```

- `type="content"`: `source` e `alt-text` obrigatórios. Borda 1px automática (`border="false"` desliga).
- `type="complex"`: além de `source` e `alt-text`, inclui descrição longa e fecha com `:::image-end:::`.
- `type="icon"`: decorativa, sem `alt-text`.
- `loc-scope` quando a imagem tem escopo de localização diferente do artigo.
- Suportados por padrão: `.jpg` e `.png`. Outros (ex. `.gif`) precisam entrar em `resource` no `docfx.json`.
- Não use o nome do arquivo como alt text.

## Includes

```md
[!INCLUDE [título](caminho/arquivo.md)]
```

- Bloco: sozinho na linha. Inline: dentro da frase (nunca para algo menor que uma frase).
- `INCLUDE` em maiúsculas e um espaço antes do título.
- Não aninhe includes. Não use include como único conteúdo do artigo.
- Pastas `/includes` não entram no build: imagens ficam em `/media`, fora delas.
- Texto do include deve ser frase completa, sem depender do texto ao redor.

## Outras extensões

| Extensão | Sintaxe |
| :--- | :--- |
| Checklist | `> [!div class="checklist"]` seguido de itens `> * item` |
| Próximo passo | `> [!div class="nextstepaction"]` seguido de `> [texto](link)` |
| Não traduzir | `:::no-loc text="Texto":::` ou `no-loc: [Termo]` no frontmatter |
| Quebra em tabela | `> [!div class="mx-tdBreakAll"]` envolvendo a tabela |
| Seletor único | `> [!div class="op_single_selector"]` com lista de links |
| Multisseletor | `> [!div class="op_multi_selector" title1="..." title2="..."]` |
| Colunas | `:::row:::` / `:::column span="":::` / `:::column-end:::` / `:::row-end:::` (desencorajado) |

- Checklist só no começo ou no fim do artigo, para resumir o que se aprende.
- `no-loc` diferencia maiúsculas de minúsculas. Escape caracteres especiais com `\`. Em `docfx.json` global ele não funciona; precisa estar em cada arquivo.
- Colunas: só Markdown básico dentro, sem títulos, tabelas ou abas.

## Tabelas de matriz de dados

Primeira célula do cabeçalho vazia e toda célula da primeira coluna em negrito, senão a tabela não é acessível:

```md
|                  | Cab. 1 | Cab. 2 |
| ---------------- | ------ | ------ |
| **Linha A**      | 1A     | 2A     |
```

## Caracteres

- Aspas curvas coladas do Word viram `â€™`. Troque por retas ou use `&#8217;` (apóstrofo), `&#8220;`/`&#8221;` (aspas duplas).
- `_` em alt text: escape com `\_`.
- Comentários HTML vão para o HTML público.

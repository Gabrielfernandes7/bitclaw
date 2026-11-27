---
name: disk-usage
description: Mostra o tamanho total de um arquivo ou diretório. Use quando o usuário perguntar sobre o espaço em disco.
---

> [!tip]
> Bitclaw skill created


# Disk usage

1. Ache o caminho do arquivo ou diretório com `find_files`, se o usuário não o deu.
2. Chame `run_script` exatamente com estes parâmetros, trocando só `<caminho>`: `{"path": "skills/disk-usage/scripts/disk-usage.sh", "args": ["<caminho>"]}`.
3. Responda com o tamanho que a saída trouxe, sem descrever a chamada.
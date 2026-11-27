# Modelos de script

## Shell

```sh
#!/bin/bash
# contar-linhas: conta as linhas de um arquivo do projeto.
set -eu
if [ $# -lt 1 ]; then
  echo "uso: contar-linhas.sh <arquivo>" >&2
  exit 2
fi
if [ ! -f "$1" ]; then
  echo "erro: '$1' não existe. Use find_files para achar o arquivo." >&2
  exit 1
fi
wc -l < "$1"
```

## Python

```python
#!/usr/bin/env python3
"""somar: soma os números passados como argumentos."""
import sys

if len(sys.argv) < 2:
    print("uso: somar.py <n1> [n2 ...]", file=sys.stderr)
    sys.exit(2)
try:
    total = sum(float(a) for a in sys.argv[1:])
except ValueError as e:
    print(f"erro: {e}. Passe só números.", file=sys.stderr)
    sys.exit(1)
print(total)
```

## SKILL.md da ferramenta

```
---
name: contar-linhas
description: Conta as linhas de um arquivo do projeto. Use quando o usuário perguntar quantas linhas um arquivo tem.
---

# Contar linhas

1. Ache o caminho do arquivo com find_files, se o usuário não o deu.
2. Chame run_script com path "skills/contar-linhas/scripts/contar-linhas.sh" e args ["<arquivo>"].
3. Responda com o número que a saída trouxe.
```

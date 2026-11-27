#!/bin/bash
# disk-usage: mostra o tamanho total de um arquivo ou diretório.
set -eu
if [ $# -lt 1 ]; then
  echo "uso: disk-usage.sh <caminho>" >&2
  exit 2
fi
if [ ! -e "$1" ]; then
  echo "erro: '$1' não existe." >&2
  exit 1
fi
du -sh "$1"
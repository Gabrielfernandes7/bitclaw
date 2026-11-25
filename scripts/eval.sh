#!/usr/bin/env bash
# Avaliação com modelos reais (Fase 5 de docs/PROJETO.md).
# Cada tarefa roda num projeto novo e o resultado é verificado no disco, não pelo texto do modelo.
# Uso: [REPEATS=3] scripts/eval.sh [modelo ...]
# Padrão: qwen2.5-coder:3b (controle) e nemotron-3-nano:4b-mlx (padrão).
# FunctionGemma fora: não validado na integração Ollama (ver docs/PROJETO.md, seção 7).
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/bitclaw"
MODELS=("$@")
if [ ${#MODELS[@]} -eq 0 ]; then
  MODELS=(qwen2.5-coder:3b nemotron-3-nano:4b-mlx)
fi
REPEATS="${REPEATS:-3}"
TASKS=(criar_txt criar_py editar ler_responder nao_alterar)
WORK="$(mktemp -d "${TMPDIR:-/tmp}/bitclaw-eval.XXXXXX")"

(cd "$ROOT" && cargo build --quiet) || { echo "build falhou"; exit 1; }

# Cada tarefa define: seed (arquivos iniciais), prompt, check (verificação no disco).
seed() {
  case "$1" in
    editar) printf 'print(soma(3, 4))\n' > calc.py ;;
    ler_responder) printf 'A capital do Brasil é Brasília.\n' > notas.txt ;;
    nao_alterar) printf 'x\n' > a.txt ;;
  esac
}
prompt() {
  case "$1" in
    criar_txt) echo "crie um arquivo hello.txt contendo exatamente a frase: ola mundo" ;;
    criar_py) echo "crie um arquivo fib.py com uma função fibonacci(n) que retorna o n-ésimo termo" ;;
    editar) echo "em calc.py troque soma(3, 4) por soma(10, 20)" ;;
    ler_responder) echo "leia notas.txt e diga qual é a capital do Brasil" ;;
    nao_alterar) echo "liste os arquivos do diretório. não altere nada" ;;
  esac
}
check() {  # $1 = tarefa, $2 = log; executado dentro do projeto
  case "$1" in
    criar_txt) grep -q "ola mundo" hello.txt 2>/dev/null ;;
    criar_py) grep -q "def fibonacci" fib.py 2>/dev/null ;;
    editar) grep -q "soma(10, 20)" calc.py 2>/dev/null && ! grep -q "soma(3, 4)" calc.py ;;
    ler_responder) grep -qi "brasília" "$2" ;;
    nao_alterar) [ "$(cat a.txt 2>/dev/null)" = "x" ] && [ "$(ls -A | tr '\n' ' ')" = "a.txt " ] ;;
  esac
}

# Saída: uma linha por execução. "erros" conta ações com erro e turnos interrompidos
# (repetição ou limite de passos), que não aparecem como [erro].
printf "%-24s %-14s %-4s %-6s %-6s %s\n" "modelo" "tarefa" "run" "result" "seg" "erros"
for model in "${MODELS[@]}"; do
  for task in "${TASKS[@]}"; do
    for run in $(seq 1 "$REPEATS"); do
      dir="$WORK/$model/$task/run$run"
      mkdir -p "$dir"
      (cd "$dir" && seed "$task")
      log="$dir.log"
      start=$(date +%s)
      printf '%s\n/sair\n' "$(prompt "$task")" | "$BIN" --dir "$dir" --model "$model" --yes >"$log" 2>&1
      secs=$(( $(date +%s) - start ))
      result=FAIL
      (cd "$dir" && check "$task" "$log") && result=PASS
      errs=$(grep -cE '\[erro\]|^interrompido:' "$log")
      printf "%-24s %-14s %-4s %-6s %-6s %s\n" "$model" "$task" "$run" "$result" "$secs" "$errs"
    done
  done
done
echo
echo "logs e projetos em: $WORK"

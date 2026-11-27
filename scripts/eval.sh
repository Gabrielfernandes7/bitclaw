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
# TASKS="navegar_cor achar_arquivo" scripts/eval.sh roda só essas.
read -ra TASKS <<< "${TASKS:-criar_txt criar_py editar ler_responder responder_fiel nao_alterar navegar_cor achar_arquivo achar_funcao corrigir_py renomear navegar_real}"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/bitclaw-eval.XXXXXX")"

(cd "$ROOT" && cargo build --quiet) || { echo "build falhou"; exit 1; }

# Projeto Rust pequeno para as tarefas de navegação: a cor está definida num só lugar (src/ui/theme.rs),
# usada em vários arquivos e também aparece em assets, documentação e na pasta de build, que distraem.
seed_projeto() {
  mkdir -p src/ui/widgets assets docs target/debug
  printf '[package]\nname = "demo"\nversion = "0.1.0"\nedition = "2024"\n' > Cargo.toml
  printf 'mod ui;\n\nfn main() {\n    ui::run();\n}\n' > src/main.rs
  printf 'pub mod theme;\npub mod widgets;\n\npub fn run() {\n    widgets::status::render();\n}\n' > src/ui/mod.rs
  printf '//! Cores da interface.\nuse ratatui::style::Color;\n\n// Destaque: títulos e avisos.\npub const ACCENT: Color = Color::Yellow;\npub const MUTED: Color = Color::Gray;\npub const OK: Color = Color::Green;\n' > src/ui/theme.rs
  printf 'pub mod status;\npub mod header;\n' > src/ui/widgets/mod.rs
  printf 'use crate::ui::theme::{ACCENT, MUTED};\n\n// Barra de status: fase atual em destaque.\npub fn render() {\n    let _ = (ACCENT, MUTED);\n}\n' > src/ui/widgets/status.rs
  printf 'use crate::ui::theme::ACCENT;\n\npub fn title() {\n    let _ = ACCENT;\n}\n' > src/ui/widgets/header.rs
  printf '<svg><rect fill="yellow"/><text fill="yellow">logo</text></svg>\n' > assets/logo.svg
  printf '# Interface\n\nO terminal usa amarelo para destaques e cinza para texto secundário.\n' > docs/ui.md
  printf 'pub const ACCENT: Color = Color::Yellow; // cópia gerada\n' > target/debug/theme.rs
  printf 'TOKEN=segredo\n' > .env
}

# Cada tarefa define: seed (arquivos iniciais), prompt, check (verificação no disco).
seed() {
  case "$1" in
    navegar_cor|achar_arquivo|achar_funcao|renomear) seed_projeto ;;
    corrigir_py) printf 'def dobro(x):\n    return x * 2\n\nprint(dobro(3)\n' > dobro.py ;;
    # O pedido real que motivou as buscas, no próprio código do BitClaw (versão do último commit).
    navegar_real) git -C "$ROOT" archive HEAD | tar -x && git init -q && git add -A && git -c user.name=eval -c user.email=eval@local commit -qm base ;;
    editar) printf 'print(soma(3, 4))\n' > calc.py ;;
    ler_responder) printf 'A capital do Brasil é Brasília.\n' > notas.txt ;;
    responder_fiel) printf 'O codigo secreto e 4817.\n' > notas.txt ;;
    nao_alterar) printf 'x\n' > a.txt ;;
  esac
}
prompt() {
  case "$1" in
    criar_txt) echo "crie um arquivo hello.txt contendo exatamente a frase: ola mundo" ;;
    criar_py) echo "crie um arquivo fib.py com uma função fibonacci(n) que retorna o n-ésimo termo" ;;
    editar) echo "em calc.py troque soma(3, 4) por soma(10, 20)" ;;
    ler_responder) echo "leia notas.txt e diga qual é a capital do Brasil" ;;
    responder_fiel) echo "leia notas.txt e diga qual é o código secreto" ;;
    nao_alterar) echo "liste os arquivos do diretório. não altere nada" ;;
    navegar_cor) echo "troque a cor amarela da interface do terminal para vermelho" ;;
    achar_arquivo) echo "qual arquivo implementa o widget de status?" ;;
    achar_funcao) echo "qual função desenha o título no widget header?" ;;
    corrigir_py) echo "dobro.py tem um erro de sintaxe; corrija-o" ;;
    renomear) echo "renomeie a constante MUTED para SECONDARY no arquivo src/ui/theme.rs" ;;
    navegar_real) echo "troque a cor amarela do terminal TUI para vermelho" ;;
  esac
}
# Resposta final: o texto depois da última linha de ferramenta ('  · ', '  ✓' ou '  ✗') do log.
answer() { awk '/^  [·✓✗] /{buf=""; next} NF{buf=buf" "$0} END{print buf}' "$1"; }
check() {  # $1 = tarefa, $2 = log; executado dentro do projeto
  case "$1" in
    criar_txt) grep -q "ola mundo" hello.txt 2>/dev/null ;;
    criar_py) grep -q "def fibonacci" fib.py 2>/dev/null ;;
    editar) grep -q "soma(10, 20)" calc.py 2>/dev/null && ! grep -q "soma(3, 4)" calc.py ;;
    ler_responder) grep -qi "brasília" "$2" ;;
    responder_fiel) answer "$2" | grep -q "4817" ;;
    nao_alterar) [ "$(cat a.txt 2>/dev/null)" = "x" ] && [ "$(ls -A | tr '\n' ' ')" = "a.txt " ] ;;
    # Só a definição muda: o resto do projeto (usos, assets, build) fica como estava.
    navegar_cor) grep -q "ACCENT: Color = Color::Red" src/ui/theme.rs && ! grep -q "Color::Yellow" src/ui/theme.rs \
      && grep -q yellow assets/logo.svg && grep -q Yellow target/debug/theme.rs ;;
    achar_arquivo) answer "$2" | grep -q "src/ui/widgets/status.rs" ;;
    achar_funcao) answer "$2" | grep -q "title" ;;
    corrigir_py) python3 -m py_compile dobro.py 2>/dev/null ;;
    renomear) grep -q "SECONDARY" src/ui/theme.rs && ! grep -q "MUTED" src/ui/theme.rs ;;
    # Só a definição do amarelo muda, no arquivo de cores.
    navegar_real) [ "$(git diff --name-only)" = "src/app/tui/theme.rs" ] && ! grep -q "255, 204, 0" src/app/tui/theme.rs ;;
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
      printf '%s\n/exit\n' "$(prompt "$task")" | "$BIN" ${CONFIG:+--config "$CONFIG"} --dir "$dir" --model "$model" --yes >"$log" 2>&1
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

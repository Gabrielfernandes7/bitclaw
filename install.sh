#!/bin/sh
# Instala o BitClaw a partir dos releases do GitHub:
#
#   curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh
#
# Variáveis opcionais:
#   BITCLAW_VERSION      versão a instalar, ex.: v0.2.0 (padrão: o último release)
#   BITCLAW_INSTALL_DIR  pasta do binário (padrão: ~/.local/bin)
#
# Desinstalar (o mesmo que `bitclaw uninstall --yes`):
#   curl -fsSL https://raw.githubusercontent.com/Gabrielfernandes7/bitclaw/main/install.sh | sh -s -- --uninstall
#
# O contrato com o release é o de src/app/update.rs: bitclaw-<target>.tar.gz e SHA256SUMS.
# Tudo fica dentro de main, chamada na última linha: um download cortado no meio não roda nada.

set -eu

REPO="Gabrielfernandes7/bitclaw"

say() { printf '%s\n' "$*"; }
fail() { printf 'bitclaw: %s\n' "$*" >&2; exit 1; }

detect_target() {
  os=$(uname -s)
  arch=$(uname -m)
  # Num Mac com Apple Silicon, um terminal sob Rosetta diz x86_64: o binário nativo é melhor.
  if [ "$os" = "Darwin" ] && [ "$arch" = "x86_64" ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = "1" ]; then
    arch=arm64
  fi
  case "$os/$arch" in
    Darwin/arm64 | Darwin/aarch64) echo aarch64-apple-darwin ;;
    Darwin/x86_64) echo x86_64-apple-darwin ;;
    Linux/x86_64 | Linux/amd64) echo x86_64-unknown-linux-gnu ;;
    Linux/aarch64 | Linux/arm64) echo aarch64-unknown-linux-gnu ;;
    *) fail "não há binário pronto para $os $arch. Com Rust instalado: cargo install --git https://github.com/$REPO" ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    fail "preciso de sha256sum ou shasum para conferir o download"
  fi
}

uninstall() {
  dir="${BITCLAW_INSTALL_DIR:-$HOME/.local/bin}"
  config="${XDG_CONFIG_HOME:-$HOME/.config}/bitclaw"
  [ -f "$dir/bitclaw" ] && rm -f "$dir/bitclaw" && say "apagado: $dir/bitclaw"
  [ -d "$config" ] && rm -rf "$config" && say "apagado: $config"
  say "as pastas .bitclaw/ dentro dos seus projetos ficam (backup, recibos e /undo)."
  say "bitclaw desinstalado."
}

main() {
  if [ "${1:-}" = "--uninstall" ]; then
    uninstall
    return
  fi
  command -v curl >/dev/null 2>&1 || fail "preciso do curl"
  command -v tar >/dev/null 2>&1 || fail "preciso do tar"

  target=$(detect_target)
  asset="bitclaw-$target.tar.gz"
  version="${BITCLAW_VERSION:-latest}"
  if [ -n "${BITCLAW_DOWNLOAD_BASE:-}" ]; then
    # Só para testar um release falso (ex.: file:///tmp/release).
    base="$BITCLAW_DOWNLOAD_BASE"
  elif [ "$version" = "latest" ]; then
    base="https://github.com/$REPO/releases/latest/download"
  else
    base="https://github.com/$REPO/releases/download/$version"
  fi
  dir="${BITCLAW_INSTALL_DIR:-$HOME/.local/bin}"

  tmp=$(mktemp -d "${TMPDIR:-/tmp}/bitclaw-install.XXXXXX")
  trap 'rm -rf "$tmp"' EXIT INT TERM

  say "baixando $asset ($version)..."
  curl -fsSL "$base/$asset" -o "$tmp/$asset" || fail "não consegui baixar $base/$asset"
  curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS" || fail "não consegui baixar $base/SHA256SUMS"

  expected=$(awk -v f="$asset" '{ n = $2; sub(/^\*/, "", n); if (n == f) print $1 }' "$tmp/SHA256SUMS")
  [ -n "$expected" ] || fail "SHA256SUMS não tem $asset"
  actual=$(sha256 "$tmp/$asset")
  [ "$expected" = "$actual" ] || fail "soma de $asset não confere (esperada $expected, obtida $actual). Nada foi instalado."

  tar -xzf "$tmp/$asset" -C "$tmp"
  [ -f "$tmp/bitclaw" ] || fail "$asset não tem o binário bitclaw"
  mkdir -p "$dir"
  # Copia para a mesma pasta e troca com mv: atômico, e um bitclaw aberto continua funcionando.
  cp "$tmp/bitclaw" "$dir/.bitclaw.install"
  chmod 755 "$dir/.bitclaw.install"
  mv -f "$dir/.bitclaw.install" "$dir/bitclaw"

  say "instalado: $dir/bitclaw ($("$dir/bitclaw" --version))"

  case ":$PATH:" in
    *":$dir:"*) ;;
    *)
      say ""
      say "$dir não está no PATH. Acrescente ao seu ~/.zshrc ou ~/.bashrc:"
      say "  export PATH=\"$dir:\$PATH\""
      ;;
  esac

  # O BitClaw conversa com um modelo local pelo Ollama. O instalador só avisa; não instala nada além dele mesmo.
  if ! command -v ollama >/dev/null 2>&1; then
    say ""
    say "o BitClaw usa modelos locais pelo Ollama, que não está instalado: https://ollama.com/download"
  elif ! curl -fsS -m 2 http://localhost:11434/api/tags >/dev/null 2>&1; then
    say ""
    say "o Ollama está instalado, mas não respondeu em localhost:11434. Abra o app ou rode: ollama serve"
  fi

  say ""
  say "pronto. Numa pasta de projeto, rode: bitclaw"
}

main "$@"

//! Copiar para a área de transferência o que foi selecionado com o mouse.
//!
//! Com o mouse capturado (para a roda rolar a conversa), o terminal não seleciona texto sozinho; a
//! interface seleciona e copia. Tenta o utilitário do sistema (`pbcopy`, `wl-copy`, `xclip`) e, sem
//! nenhum, manda a sequência OSC 52, que a maioria dos terminais (e o tmux, e o SSH) entende.

use std::io::{self, Write};
use std::process::{Command, Stdio};

// Copia `text`. Devolve por onde foi, para o aviso na tela.
pub(super) fn copy(text: &str) -> &'static str {
    let tools: &[(&str, &[&str])] = &[("pbcopy", &[]), ("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])];
    for (tool, args) in tools {
        if pipe(tool, args, text).is_ok() {
            return "copiado";
        }
    }
    let _ = osc52(text);
    "copiado pelo terminal"
}

fn pipe(tool: &str, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(tool).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()?;
    child.stdin.take().ok_or_else(|| io::Error::other("sem stdin"))?.write_all(text.as_bytes())?;
    if child.wait()?.success() { Ok(()) } else { Err(io::Error::other("falhou")) }
}

fn osc52(text: &str) -> io::Result<()> {
    let mut out = io::stdout();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()))?;
    out.flush()
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64("olá".as_bytes()), "b2zDoQ==");
    }
}

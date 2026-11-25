//! Leitura de respostas em streaming, linha a linha, com prazo: comum a todos os adaptadores HTTP.

use std::io::{BufRead, BufReader, Read};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

// Teto de uma chamada inteira ao modelo e tempo máximo sem receber nem uma linha do streaming.
pub(crate) const MAX_CALL: Duration = Duration::from_secs(3600);
const STALL: Duration = Duration::from_secs(300);
// De quanto em quanto tempo a espera por texto confere se o turno foi cancelado.
const TICK: Duration = Duration::from_millis(200);

// Linhas da resposta. Uma thread lê; aqui se espera com prazo para notar um provedor travado. `Ok(None)`
// significa "ainda não chegou nada": o chamador o transforma em `Chunk::Idle` para poder cancelar a espera.
// Se a leitura parar antes do fim, o canal fecha, a thread sai e a conexão cai.
pub(crate) fn stream(
    resp: impl Read + Send + 'static,
    who: &'static str,
) -> impl Iterator<Item = Result<Option<String>, String>> {
    let (tx, rx) = mpsc::channel();
    let reader = BufReader::new(resp);
    thread::spawn(move || {
        for line in reader.lines() {
            let item = line.map_err(|e| format!("a resposta do {who} foi cortada: {e}"));
            if tx.send(item).is_err() {
                break;
            }
        }
    });
    // A espera acorda a cada `TICK` para o sink conferir o cancelamento: sem isso, o Esc só valeria
    // depois do primeiro texto, e um modelo que ainda carrega ou processa o prompt levaria até
    // `STALL` para ser interrompido.
    let mut waited = Duration::ZERO;
    std::iter::from_fn(move || match rx.recv_timeout(TICK) {
        Ok(item) => {
            waited = Duration::ZERO;
            Some(item.map(Some))
        }
        Err(RecvTimeoutError::Timeout) => {
            waited += TICK;
            if waited >= STALL {
                Some(Err(format!("o {who} ficou {} s sem responder", STALL.as_secs())))
            } else {
                Some(Ok(None))
            }
        }
        Err(RecvTimeoutError::Disconnected) => None,
    })
}

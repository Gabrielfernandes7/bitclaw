//! Detector de laço: o modelo repetindo o mesmo trecho sem parar.

use super::model::Chunk;
use std::collections::HashMap;

// Fim de turno quando o modelo repete o mesmo trecho sem parar.
pub const LOOPING: &str =
    "o modelo entrou em laço e repetiu o mesmo trecho; interrompi a geração. Reformule o pedido ou desligue o raciocínio com /think.";

// Uma linha com pelo menos isto de letras e dígitos, repetida este número de vezes, é laço. Contar só
// letras e dígitos deixa de fora separadores como `| --- | --- |` e `=======`, que se repetem à toa.
pub(super) const LOOP_MIN_ALNUM: usize = 20;
// No raciocínio, repetir uma linha longa 3 vezes já é sinal de laço. Na resposta final é mais comum
// repetir de verdade (linhas de código, itens parecidos), então pede mais.
pub(super) const LOOP_REPEATS_THINKING: usize = 3;
pub(super) const LOOP_REPEATS_CONTENT: usize = 5;
// O fim do texto, com este tamanho, já ter aparecido este número de vezes também é laço: é um bloco se repetindo.
pub(super) const LOOP_TAIL_CHARS: usize = 160;
pub(super) const LOOP_TAIL_REPEATS: usize = 3;
// De quantos em quantos bytes novos o texto é examinado.
const LOOP_CHECK_EVERY: usize = 200;

// O modelo está repetindo o mesmo trecho sem parar? Olha linhas iguais e blocos que se repetem no fim.
//
// As repetições não precisam ser seguidas: no laço real que medimos, o modelo chega à resposta,
// duvida e recomeça a conta, repetindo as mesmas linhas espalhadas pelo texto (a maior repetição
// seguida era de 1 linha). Exigir linhas consecutivas deixaria esse laço passar.
pub fn is_looping(text: &str, line_repeats: usize) -> bool {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for line in text.lines() {
        let line = line.trim().to_lowercase();
        if line.chars().filter(|c| c.is_alphanumeric()).count() < LOOP_MIN_ALNUM {
            continue;
        }
        let count = seen.entry(line).or_insert(0);
        *count += 1;
        if *count >= line_repeats {
            return true;
        }
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() >= LOOP_TAIL_CHARS * LOOP_TAIL_REPEATS {
        let tail: String = chars[chars.len() - LOOP_TAIL_CHARS..].iter().collect();
        return text.matches(&tail).count() >= LOOP_TAIL_REPEATS;
    }
    false
}

// Acompanha o texto que chega do modelo e avisa quando ele entra em laço.
#[derive(Default)]
pub(super) struct LoopGuard {
    thinking: String,
    content: String,
    unchecked: usize,
    pub(super) looping: bool,
}

impl LoopGuard {
    // Devolve `true` se o texto até aqui já é um laço.
    pub(super) fn feed(&mut self, chunk: Chunk) -> bool {
        let text = match chunk {
            Chunk::Thinking(t) => {
                self.thinking.push_str(t);
                t
            }
            Chunk::Content(t) => {
                self.content.push_str(t);
                t
            }
            Chunk::Idle => return self.looping,
        };
        self.unchecked += text.len();
        if self.unchecked >= LOOP_CHECK_EVERY {
            self.unchecked = 0;
            self.looping = is_looping(&self.thinking, LOOP_REPEATS_THINKING) || is_looping(&self.content, LOOP_REPEATS_CONTENT);
        }
        self.looping
    }
}

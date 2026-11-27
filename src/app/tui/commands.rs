//! Comandos com `/` e o menu que os lista.

// Comandos com `/`. O nome é em inglês, como no Claude Code; os nomes antigos em português
// continuam funcionando como apelidos.
pub(super) struct Cmd {
    pub(super) name: &'static str,
    pub(super) aliases: &'static [&'static str],
    pub(super) desc: &'static str,
    // Argumentos aceitos, mostrados ao lado do nome no menu: `[a|b]` escolhe um, `<x>` é obrigatório.
    pub(super) usage: &'static str,
}

pub(super) const COMMANDS: [Cmd; 18] = [
    Cmd { name: "/help", aliases: &[], desc: "mostra os comandos e as teclas", usage: "" },
    Cmd { name: "/model", aliases: &[], desc: "troca o modelo: load <nome>, show [nome] ou busca entre os instalados", usage: "<load|show> [nome]" },
    Cmd { name: "/effort", aliases: &[], desc: "esforço de raciocínio: só os níveis que o modelo aceita", usage: "<off|low|medium|high>" },
    Cmd { name: "/context", aliases: &[], desc: "janela de contexto em tokens: mostra, troca (16k, 32768) ou volta ao padrão (reset)", usage: "<tokens|reset>" },
    Cmd { name: "/temp", aliases: &["/temperature"], desc: "temperatura de geração: mostra, troca (0 a 2) ou volta ao padrão (reset)", usage: "<0-2|reset>" },
    Cmd { name: "/init", aliases: &[], desc: "cria o AGENTS.md com o resumo verificável do projeto", usage: "" },
    Cmd { name: "/plan", aliases: &[], desc: "modo Plan: só leitura, nada é alterado nem executado. Sem argumento, alterna", usage: "[on|off]" },
    Cmd { name: "/soul", aliases: &[], desc: "mostra o SOUL.md em vigor (quem o agente é para você); `bitclaw soul` refaz as perguntas", usage: "" },
    Cmd { name: "/clear", aliases: &[], desc: "apaga o histórico e mantém o prompt do sistema", usage: "" },
    Cmd { name: "/think", aliases: &[], desc: "liga ou desliga o raciocínio do modelo (gasta mais contexto)", usage: "" },
    Cmd { name: "/skills", aliases: &[], desc: "abre as skills para ligar e desligar", usage: "" },
    Cmd { name: "/skill", aliases: &[], desc: "baixa uma skill de um repositório git: install <dono/repo> [--path pasta], depois yes ou no", usage: "<install|yes|no> [dono/repo]" },
    Cmd { name: "/openrouter", aliases: &[], desc: "configura o OpenRouter (chave, modelo, agente) como em `bitclaw openrouter` e reabre com ele", usage: "" },
    Cmd { name: "/status", aliases: &[], desc: "versão, modelo, contexto e estatísticas da sessão", usage: "" },
    Cmd { name: "/undo", aliases: &[], desc: "desfaz as alterações do último turno (arquivos criados, editados ou removidos)", usage: "" },
    Cmd { name: "/restart-dev", aliases: &[], desc: "recompila e reabre o BitClaw com as alterações do código (desenvolvimento)", usage: "" },
    Cmd { name: "/welcome-dev", aliases: &[], desc: "reabre o BitClaw como na primeira vez, com a tela de boas-vindas (não apaga nada)", usage: "" },
    Cmd { name: "/exit", aliases: &["/quit"], desc: "encerra o BitClaw", usage: "" },
];

impl Cmd {
    pub(super) fn is(&self, text: &str) -> bool {
        self.name == text || self.aliases.contains(&text)
    }

    pub(super) fn starts_with(&self, prefix: &str) -> bool {
        self.name.starts_with(prefix) || self.aliases.iter().any(|a| a.starts_with(prefix))
    }
}

// Comandos que o menu e o `/help` mostram. `/restart-dev` só aparece com o código-fonte por perto:
// num binário instalado ele não tem o que recompilar. Digitado mesmo assim, explica por quê.
pub(super) fn visible() -> impl Iterator<Item = &'static Cmd> {
    visible_with(super::restart::available())
}

fn visible_with(has_source: bool) -> impl Iterator<Item = &'static Cmd> {
    COMMANDS.iter().filter(move |c| c.name != "/restart-dev" || has_source)
}

// Comandos que aceitam um argumento depois do nome (`/model <id>`, `/effort <nível>`).
pub(super) const ARG_COMMANDS: &[&str] = &["/model", "/effort", "/context", "/temp", "/skill", "/plan"];

// O que um comando de ajuste (`/context`, `/temp`) pede: mostrar o valor, voltar ao padrão ou trocar.
#[derive(Debug, PartialEq)]
pub(super) enum Setting<T> {
    Show,
    Reset,
    Set(T),
}

// Janela mínima e máxima aceitas. Abaixo do mínimo, o prompt do sistema e as ferramentas já ocupam quase tudo.
pub(super) const MIN_CONTEXT: usize = 2048;
pub(super) const MAX_CONTEXT: usize = 1_048_576;

fn is_reset(word: &str) -> bool {
    matches!(word, "reset" | "padrão" | "padrao" | "default")
}

// `/context`: um número de tokens (`16384`) ou em milhares (`16k`, que vale 16 × 1024).
pub(super) fn parse_context(arg: &str) -> Result<Setting<usize>, String> {
    let arg = arg.trim().to_lowercase();
    if arg.is_empty() {
        return Ok(Setting::Show);
    }
    if is_reset(&arg) {
        return Ok(Setting::Reset);
    }
    let (digits, factor) = match arg.strip_suffix('k') {
        Some(d) => (d.trim(), 1024),
        None => (arg.as_str(), 1),
    };
    let invalid = || format!("contexto inválido: '{arg}'. Use um número de tokens, como 16384 ou 16k.");
    let tokens = digits.parse::<usize>().ok().and_then(|n| n.checked_mul(factor)).ok_or_else(invalid)?;
    if !(MIN_CONTEXT..=MAX_CONTEXT).contains(&tokens) {
        return Err(format!("o contexto fica entre {MIN_CONTEXT} e {MAX_CONTEXT} tokens."));
    }
    Ok(Setting::Set(tokens))
}

// `/temp`: de 0 a 2, com ponto ou vírgula (`0.3`, `0,3`).
pub(super) fn parse_temperature(arg: &str) -> Result<Setting<f32>, String> {
    let arg = arg.trim().to_lowercase();
    if arg.is_empty() {
        return Ok(Setting::Show);
    }
    if is_reset(&arg) {
        return Ok(Setting::Reset);
    }
    match arg.replace(',', ".").parse::<f32>() {
        Ok(t) if (0.0..=2.0).contains(&t) => Ok(Setting::Set(t)),
        _ => Err(format!("temperatura inválida: '{arg}'. Use um valor de 0 a 2, como 0.1 ou 0.8.")),
    }
}

// O que a temperatura faz nesta faixa, numa frase.
pub(super) fn temperature_effect(t: f32) -> &'static str {
    if t < 0.3 {
        "baixa: respostas estáveis e repetíveis, boas para ferramentas e código; pode repetir-se."
    } else if t <= 0.9 {
        "média: equilíbrio entre estabilidade e variação."
    } else {
        "alta: respostas mais variadas, com mais erro em argumentos de ferramentas."
    }
}

// O que `/model` pede. `load` e `show` são subcomandos; um nome solto continua valendo como `load`.
#[derive(Debug, PartialEq)]
pub(super) enum ModelAction {
    // Sem argumento: abre o painel de escolha.
    Pick,
    Load(String),
    // Sem nome, mostra o modelo ativo.
    Show(Option<String>),
}

pub(super) fn parse_model_arg(arg: &str) -> ModelAction {
    let arg = arg.trim();
    let (word, rest) = arg.split_once(char::is_whitespace).map_or((arg, ""), |(w, r)| (w, r.trim()));
    match word {
        "" => ModelAction::Pick,
        "load" if rest.is_empty() => ModelAction::Pick,
        "load" => ModelAction::Load(rest.to_string()),
        "show" => ModelAction::Show((!rest.is_empty()).then(|| rest.to_string())),
        _ => ModelAction::Load(arg.to_string()),
    }
}

// Subcomandos de `/model`, para destacar no editor.
pub(super) const MODEL_SUBCOMMANDS: &[&str] = &["load", "show"];

// Trechos da primeira linha que são palavras reservadas: o nome do comando e, em `/model`, o
// subcomando. Devolve intervalos em caracteres, `(início, fim)`, para o editor destacar.
pub(super) fn keyword_ranges(line: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    if !line.starts_with('/') {
        return out;
    }
    let head_len = line.find(char::is_whitespace).unwrap_or(line.len());
    let head = &line[..head_len];
    let Some(cmd) = COMMANDS.iter().find(|c| c.is(head)) else { return out };
    out.push((0, head.chars().count()));
    if cmd.name == "/model" {
        let after = &line[head_len..];
        let word = after.trim_start();
        let skipped = after.len() - word.len();
        let sub_len = word.find(char::is_whitespace).unwrap_or(word.len());
        if MODEL_SUBCOMMANDS.contains(&&word[..sub_len]) {
            let start = line[..head_len + skipped].chars().count();
            out.push((start, start + word[..sub_len].chars().count()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_arguments_parse_into_actions() {
        assert_eq!(parse_model_arg(""), ModelAction::Pick);
        assert_eq!(parse_model_arg("load"), ModelAction::Pick);
        assert_eq!(parse_model_arg("load qwen3:4b"), ModelAction::Load("qwen3:4b".into()));
        assert_eq!(parse_model_arg("  load   qwen3:4b "), ModelAction::Load("qwen3:4b".into()));
        assert_eq!(parse_model_arg("show"), ModelAction::Show(None));
        assert_eq!(parse_model_arg("show qwen3:4b"), ModelAction::Show(Some("qwen3:4b".into())));
        assert_eq!(parse_model_arg("qwen3:4b"), ModelAction::Load("qwen3:4b".into()), "nome solto segue valendo");
    }

    #[test]
    fn context_accepts_tokens_thousands_and_reset() {
        assert_eq!(parse_context(""), Ok(Setting::Show));
        assert_eq!(parse_context("16384"), Ok(Setting::Set(16384)));
        assert_eq!(parse_context("16k"), Ok(Setting::Set(16384)));
        assert_eq!(parse_context(" 32K "), Ok(Setting::Set(32768)));
        assert_eq!(parse_context("reset"), Ok(Setting::Reset));
        assert_eq!(parse_context("padrão"), Ok(Setting::Reset));
        assert!(parse_context("abc").is_err());
        assert!(parse_context("100").unwrap_err().contains("entre"), "abaixo do mínimo");
        assert!(parse_context("99999999999999999999").is_err(), "estouro vira erro, não pânico");
    }

    #[test]
    fn temperature_takes_a_dot_or_a_comma_between_zero_and_two() {
        assert_eq!(parse_temperature(""), Ok(Setting::Show));
        assert_eq!(parse_temperature("0.3"), Ok(Setting::Set(0.3)));
        assert_eq!(parse_temperature("0,3"), Ok(Setting::Set(0.3)));
        assert_eq!(parse_temperature("2"), Ok(Setting::Set(2.0)));
        assert_eq!(parse_temperature("reset"), Ok(Setting::Reset));
        assert!(parse_temperature("2.5").is_err());
        assert!(parse_temperature("-0.1").is_err());
        assert!(parse_temperature("quente").is_err());
        assert!(temperature_effect(0.1).starts_with("baixa") && temperature_effect(0.6).starts_with("média"));
        assert!(temperature_effect(1.2).starts_with("alta"));
    }

    #[test]
    fn keywords_are_the_command_and_the_model_subcommand() {
        assert_eq!(keyword_ranges("/model"), vec![(0, 6)]);
        assert_eq!(keyword_ranges("/model load x"), vec![(0, 6), (7, 11)]);
        assert_eq!(keyword_ranges("/model   show"), vec![(0, 6), (9, 13)]);
        assert_eq!(keyword_ranges("/model qwen"), vec![(0, 6)]);
        assert_eq!(keyword_ranges("/quit"), vec![(0, 5)], "apelido também é palavra reservada");
        assert_eq!(keyword_ranges("/efort"), vec![], "comando desconhecido não é destacado");
        assert_eq!(keyword_ranges("model load"), vec![], "sem a barra, é texto comum");
        assert_eq!(keyword_ranges("/effort load"), vec![(0, 7)], "load só é subcomando de /model");
    }

    #[test]
    fn restart_dev_is_listed_only_with_the_source_around() {
        let names = |src| visible_with(src).map(|c| c.name).collect::<Vec<_>>();
        assert!(names(true).contains(&"/restart-dev"));
        assert!(!names(false).contains(&"/restart-dev"));
        assert_eq!(names(false).len(), COMMANDS.len() - 1, "só ele sai");
    }
}

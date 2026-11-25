//! Comandos com `/` e o menu que os lista.

// Comandos com `/`. O nome é em inglês, como no Claude Code; os nomes antigos em português
// continuam funcionando como apelidos.
pub(super) struct Cmd {
    pub(super) name: &'static str,
    pub(super) aliases: &'static [&'static str],
    pub(super) desc: &'static str,
}

pub(super) const COMMANDS: [Cmd; 8] = [
    Cmd { name: "/help", aliases: &[], desc: "mostra os comandos e as teclas" },
    Cmd { name: "/model", aliases: &[], desc: "troca o modelo: load <nome>, show [nome] ou busca entre os instalados" },
    Cmd { name: "/effort", aliases: &[], desc: "esforço de raciocínio: só os níveis que o modelo aceita" },
    Cmd { name: "/init", aliases: &[], desc: "cria o AGENTS.md com o resumo verificável do projeto" },
    Cmd { name: "/clear", aliases: &[], desc: "apaga o histórico e mantém o prompt do sistema" },
    Cmd { name: "/think", aliases: &[], desc: "liga ou desliga o raciocínio do modelo (gasta mais contexto)" },
    Cmd { name: "/skills", aliases: &[], desc: "abre as skills para ligar e desligar" },
    Cmd { name: "/exit", aliases: &["/quit"], desc: "encerra o BitClaw" },
];

impl Cmd {
    pub(super) fn is(&self, text: &str) -> bool {
        self.name == text || self.aliases.contains(&text)
    }

    pub(super) fn starts_with(&self, prefix: &str) -> bool {
        self.name.starts_with(prefix) || self.aliases.iter().any(|a| a.starts_with(prefix))
    }
}

// Comandos que aceitam um argumento depois do nome (`/model <id>`, `/effort <nível>`).
pub(super) const ARG_COMMANDS: &[&str] = &["/model", "/effort"];

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
}

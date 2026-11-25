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
    Cmd { name: "/model", aliases: &[], desc: "troca o modelo: busca e seleção entre os instalados" },
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

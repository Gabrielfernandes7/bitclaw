//! Modelos e suas capacidades, sem depender de provedor nem de interface.
//!
//! O catálogo diz *o que* existe e *o que* cada modelo sabe fazer. O adaptador (ex.: `ollama.rs`)
//! traduz isso para a API concreta. A TUI só consulta este módulo para decidir o que mostrar.
//!
//! Seleção de modelo e esforço de raciocínio são dimensões separadas: trocar um não muda o outro,
//! mas o esforço é sempre validado contra as capacidades do modelo ativo.

// Esforço compartilhado entre a interface, o agente e o cliente: o `/effort` escreve, a próxima requisição lê.
pub type SharedEffort = std::sync::Arc<std::sync::Mutex<Effort>>;

// Esforço de raciocínio pedido pelo usuário. `On` é o liga/desliga de modelos que não têm níveis;
// `Off` vale para qualquer modelo e significa "sem raciocínio".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Off,
    On,
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Off => "off",
            Effort::On => "on",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
        }
    }

    pub fn parse(text: &str) -> Option<Effort> {
        match text {
            "off" => Some(Effort::Off),
            "on" => Some(Effort::On),
            "low" => Some(Effort::Low),
            "medium" => Some(Effort::Medium),
            "high" => Some(Effort::High),
            _ => None,
        }
    }

    pub fn is_off(self) -> bool {
        self == Effort::Off
    }

    // Posição numa escala, usada para escolher o nível mais próximo quando o modelo não tem o pedido.
    fn rank(self) -> u8 {
        match self {
            Effort::Off => 0,
            Effort::Low => 1,
            Effort::On | Effort::Medium => 2,
            Effort::High => 3,
        }
    }
}

// O que o modelo faz com o raciocínio. "Liga/desliga" e "níveis" são coisas diferentes: um modelo
// de níveis também aceita desligar, mas não aceita um `true` genérico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reasoning {
    Unsupported,
    Toggle,
    Levels(Vec<Effort>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub tool_calling: bool,
    pub reasoning: Reasoning,
    // Janela máxima que o modelo aceita. `None` quando o provedor não informa.
    pub context_window: Option<usize>,
}

impl ModelCapabilities {
    // Quando nada se sabe sobre o modelo: o agente precisa de ferramentas, então assume que tem,
    // e não assume raciocínio (é o padrão seguro: não envia parâmetro que talvez não exista).
    pub fn unknown() -> Self {
        Self { tool_calling: true, reasoning: Reasoning::Unsupported, context_window: None }
    }

    // Níveis que o modelo aceita. `Off` está sempre na lista.
    pub fn supported_efforts(&self) -> Vec<Effort> {
        match &self.reasoning {
            Reasoning::Unsupported => vec![Effort::Off],
            Reasoning::Toggle => vec![Effort::Off, Effort::On],
            Reasoning::Levels(levels) => std::iter::once(Effort::Off).chain(levels.iter().copied()).collect(),
        }
    }

    pub fn supports(&self, effort: Effort) -> bool {
        self.supported_efforts().contains(&effort)
    }

    // Valor para "liga o raciocínio" ao iniciar: o nível médio quando há níveis, o único nível se houver um.
    pub fn default_on(&self) -> Effort {
        match &self.reasoning {
            Reasoning::Unsupported => Effort::Off,
            Reasoning::Toggle => Effort::On,
            Reasoning::Levels(levels) => {
                if levels.contains(&Effort::Medium) {
                    Effort::Medium
                } else {
                    levels.last().copied().unwrap_or(Effort::Off)
                }
            }
        }
    }

    // Valida um pedido explícito do usuário. A mensagem de erro diz quais níveis existem.
    pub fn validate(&self, wanted: Effort) -> Result<Effort, String> {
        if self.supports(wanted) {
            return Ok(wanted);
        }
        let names: Vec<&str> = self.supported_efforts().into_iter().map(Effort::as_str).collect();
        Err(format!("este modelo não aceita esforço '{}'. Níveis disponíveis: {}", wanted.as_str(), names.join(", ")))
    }

    // Interpreta o texto de `/effort <nível>` contra as capacidades do modelo.
    pub fn parse_for(&self, text: &str) -> Result<Effort, String> {
        let wanted = Effort::parse(text).ok_or_else(|| {
            format!("nível desconhecido: '{text}'. Use off, low, medium ou high.")
        })?;
        self.validate(wanted)
    }

    // O que `/think` faz: se está desligado, liga no padrão do modelo; se está ligado, desliga.
    pub fn toggle_think(&self, current: Effort) -> Effort {
        if current.is_off() { self.default_on() } else { Effort::Off }
    }

    // Ajusta um esforço já escolhido ao trocar de modelo. Nunca falha: vai para o nível suportado mais próximo.
    pub fn fit(&self, current: Effort) -> Effort {
        if self.supports(current) {
            return current;
        }
        match &self.reasoning {
            Reasoning::Unsupported => Effort::Off,
            Reasoning::Toggle => Effort::On,
            Reasoning::Levels(levels) => {
                if current.is_off() {
                    return Effort::Off;
                }
                // Empate vai para o nível mais baixo: é o lado seguro em custo e latência.
                levels
                    .iter()
                    .copied()
                    .min_by_key(|l| (l.rank().abs_diff(current.rank()), l.rank()))
                    .unwrap_or(Effort::Off)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub capabilities: ModelCapabilities,
}

impl ModelInfo {
    // Modelo que não está no catálogo (ex.: ainda não instalado): capacidades desconhecidas, nada presumido.
    pub fn unknown(id: &str) -> Self {
        Self { id: id.to_string(), provider: String::new(), capabilities: ModelCapabilities::unknown() }
    }
}

// Lista os modelos disponíveis em um provedor. Cada provedor implementa a sua.
pub trait ModelCatalog {
    fn list(&self) -> Result<Vec<ModelInfo>, String>;

    // Busca um modelo pelo id exato. Modelo ausente vira `None`, não erro: o chamador decide.
    fn find(&self, id: &str) -> Result<Option<ModelInfo>, String> {
        Ok(self.list()?.into_iter().find(|m| m.id == id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels() -> ModelCapabilities {
        ModelCapabilities {
            tool_calling: true,
            reasoning: Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High]),
            context_window: None,
        }
    }

    fn toggle() -> ModelCapabilities {
        ModelCapabilities { tool_calling: true, reasoning: Reasoning::Toggle, context_window: None }
    }

    fn plain() -> ModelCapabilities {
        ModelCapabilities { tool_calling: true, reasoning: Reasoning::Unsupported, context_window: None }
    }

    #[test]
    fn each_kind_of_model_lists_only_its_own_levels() {
        assert_eq!(plain().supported_efforts(), vec![Effort::Off]);
        assert_eq!(toggle().supported_efforts(), vec![Effort::Off, Effort::On]);
        assert_eq!(levels().supported_efforts(), vec![Effort::Off, Effort::Low, Effort::Medium, Effort::High]);
    }

    #[test]
    fn validation_rejects_levels_the_model_does_not_have() {
        assert_eq!(levels().validate(Effort::High), Ok(Effort::High));
        let err = toggle().validate(Effort::High).unwrap_err();
        assert!(err.contains("off, on"), "{err}");
        assert!(plain().validate(Effort::Low).is_err());
        assert_eq!(plain().validate(Effort::Off), Ok(Effort::Off), "desligar é sempre permitido");
    }

    #[test]
    fn switching_models_keeps_a_supported_effort_and_clamps_the_rest() {
        assert_eq!(levels().fit(Effort::Low), Effort::Low);
        assert_eq!(levels().fit(Effort::On), Effort::Medium, "liga/desliga cai no nível do meio");
        assert_eq!(toggle().fit(Effort::High), Effort::On);
        assert_eq!(plain().fit(Effort::High), Effort::Off, "sem raciocínio, nada fica ligado");
        assert_eq!(levels().fit(Effort::Off), Effort::Off);
    }

    #[test]
    fn default_on_is_safe_for_each_kind() {
        assert_eq!(plain().default_on(), Effort::Off);
        assert_eq!(toggle().default_on(), Effort::On);
        assert_eq!(levels().default_on(), Effort::Medium);
        let only_high = ModelCapabilities {
            tool_calling: true,
            reasoning: Reasoning::Levels(vec![Effort::High]),
            context_window: None,
        };
        assert_eq!(only_high.default_on(), Effort::High);
    }

    #[test]
    fn effort_names_round_trip() {
        for e in [Effort::Off, Effort::On, Effort::Low, Effort::Medium, Effort::High] {
            assert_eq!(Effort::parse(e.as_str()), Some(e));
        }
        assert_eq!(Effort::parse("maximo"), None);
    }

    #[test]
    fn effort_text_is_checked_against_the_model() {
        assert_eq!(levels().parse_for("low"), Ok(Effort::Low));
        assert!(toggle().parse_for("high").is_err());
        assert!(levels().parse_for("maximo").unwrap_err().contains("maximo"));
    }

    #[test]
    fn think_toggles_between_off_and_the_models_default() {
        assert_eq!(levels().toggle_think(Effort::Off), Effort::Medium);
        assert_eq!(levels().toggle_think(Effort::High), Effort::Off);
        assert_eq!(plain().toggle_think(Effort::Off), Effort::Off, "sem raciocínio, ligar não faz nada");
    }

    #[test]
    fn unknown_models_get_tools_and_no_reasoning() {
        let caps = ModelCapabilities::unknown();
        assert!(caps.tool_calling);
        assert_eq!(caps.supported_efforts(), vec![Effort::Off]);
    }
}

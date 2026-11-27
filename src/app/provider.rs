//! Escolha do provedor: o único lugar do projeto que sabe quais adaptadores existem.
//! Lê `[model] provider` e monta catálogo, opções e a fábrica de clientes do adaptador certo.

use bitclaw::adapters::openrouter::{self, ApiKey, OpenRouterCatalog, OpenRouterSettings};
use bitclaw::agent::ChatModel;
use bitclaw::config::AgentConfig;
use bitclaw::models::{ModelCatalog, ModelInfo, SharedEffort};
use bitclaw::ollama::{self, OllamaCatalog, OllamaSettings};
use bitclaw::core::options::{Options, SharedOptions};
use std::sync::{Arc, Mutex};

pub const PROVIDERS: &[&str] = &["ollama", "openrouter"];

#[derive(Clone)]
enum Kind {
    Ollama(OllamaSettings),
    OpenRouter(OpenRouterSettings),
}

// Tudo o que o resto do app precisa de um provedor, sem saber qual é.
#[derive(Clone)]
pub struct Provider {
    kind: Kind,
    pub catalog: Arc<dyn ModelCatalog + Send + Sync>,
    // Janela e temperatura em uso (a interface as muda) e as da configuração (para `reset`).
    pub options: SharedOptions,
    pub default_options: Options,
    // Avisos para o usuário na abertura (modelo ausente, dados saindo da máquina...).
    pub notices: Vec<String>,
}

impl Provider {
    // Monta o provedor da configuração. `url` (a flag `--url`) vale mais que o arquivo.
    pub fn connect(cfg: &AgentConfig, url: Option<String>) -> Result<Self, String> {
        let name = cfg.model.provider.as_deref().unwrap_or("ollama");
        match name {
            "ollama" => Self::ollama(cfg, url),
            "openrouter" => Self::openrouter(cfg, url),
            other => Err(format!("configuração: provedor desconhecido '{other}'. Disponíveis: {}", PROVIDERS.join(", "))),
        }
    }

    fn ollama(cfg: &AgentConfig, url: Option<String>) -> Result<Self, String> {
        let mut settings = OllamaSettings::from_config(cfg)?;
        if let Some(url) = url {
            settings.url = url;
        }
        let default_options = settings.options(cfg.model.context_window);
        let provider = Self {
            catalog: Arc::new(OllamaCatalog::new(settings.url.clone())),
            options: Arc::new(Mutex::new(default_options)),
            default_options,
            notices: Vec::new(),
            kind: Kind::Ollama(settings),
        };
        // Sem o Ollama o agente não faz nada útil: falha já na inicialização.
        let probe = provider.ollama_client(&ModelInfo::unknown(&cfg.model.name), &Arc::new(Mutex::new(bitclaw::models::Effort::Off)))?;
        let mut provider = provider;
        match probe.has_model() {
            Ok(true) => {}
            Ok(false) => provider
                .notices
                .push(format!("aviso: o modelo '{0}' não aparece no Ollama. Instale com: ollama pull {0}", cfg.model.name)),
            Err(e) => return Err(format!("{e}. Inicie o Ollama com: ollama serve")),
        }
        Ok(provider)
    }

    fn openrouter(cfg: &AgentConfig, url: Option<String>) -> Result<Self, String> {
        let mut settings = OpenRouterSettings::from_config(cfg)?;
        if let Some(url) = url {
            settings.base_url = url;
        }
        // A chave é conferida já na partida: o erro diz qual variável falta.
        let key = ApiKey::from_env(&settings.api_key_env)?;
        let default_options = settings.options(cfg.model.context_window);
        Ok(Self {
            catalog: Arc::new(OpenRouterCatalog::new(settings.base_url.clone(), Some(key))),
            options: Arc::new(Mutex::new(default_options)),
            default_options,
            notices: vec![
                "aviso: o OpenRouter é um provedor remoto: o conteúdo dos arquivos que o agente ler sai desta máquina".into(),
            ],
            kind: Kind::OpenRouter(settings),
        })
    }

    fn ollama_client(&self, info: &ModelInfo, effort: &SharedEffort) -> Result<ollama::Client, String> {
        let Kind::Ollama(settings) = &self.kind else { return Err("provedor não é o Ollama".into()) };
        ollama::Client::new(
            settings.url.clone(),
            info.id.clone(),
            Arc::clone(&self.options),
            info.capabilities.clone(),
            Arc::clone(effort),
        )
    }

    // Cliente para um modelo, com o esforço compartilhado. Cada troca de modelo é um cliente novo.
    pub fn client(&self, info: &ModelInfo, effort: &SharedEffort) -> Result<Box<dyn ChatModel>, String> {
        match &self.kind {
            Kind::Ollama(_) => Ok(Box::new(self.ollama_client(info, effort)?)),
            Kind::OpenRouter(settings) => Ok(Box::new(openrouter::Client::new(
                settings.base_url.clone(),
                ApiKey::from_env(&settings.api_key_env)?,
                settings.api_key_env.clone(),
                info.id.clone(),
                Arc::clone(&self.options),
                info.capabilities.clone(),
                Arc::clone(effort),
            )?)),
        }
    }

    // Nome do provedor como aparece no cabeçalho.
    pub fn label(&self) -> &'static str {
        match &self.kind {
            Kind::Ollama(_) => "Ollama",
            Kind::OpenRouter(_) => "OpenRouter",
        }
    }

    // O que o provedor sabe dizer do modelo (tamanho e quantização, no Ollama). Vazio se não sabe.
    pub fn model_details(&self, info: &ModelInfo) -> String {
        if let Kind::Ollama(_) = &self.kind {
            let effort = Arc::new(Mutex::new(bitclaw::models::Effort::Off));
            if let Ok(d) = self.ollama_client(info, &effort).and_then(|c| c.model_details()) {
                return format!("{} · {}", d.parameter_size.unwrap_or_else(|| "?".into()), d.quantization.unwrap_or_else(|| "?".into()));
            }
        }
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(model_extra: &str, rest: &str) -> AgentConfig {
        AgentConfig::from_toml(&format!(
            "name = \"t\"\nsystem_prompt = \"oi\"\ntools = []\n[model]\nname = \"m\"\n{model_extra}\n{rest}"
        ))
        .unwrap()
    }

    #[test]
    fn an_unknown_provider_lists_the_valid_ones() {
        let err = Provider::connect(&cfg("provider = \"openruter\"", ""), None).err().unwrap();
        assert!(err.contains("openruter") && err.contains("openrouter"), "{err}");
    }

    #[test]
    fn openrouter_without_the_key_variable_names_it() {
        let c = cfg("provider = \"openrouter\"", "[openrouter]\napi_key_env = \"BITCLAW_TESTE_SEM_CHAVE\"\n");
        let err = Provider::connect(&c, None).err().unwrap();
        assert!(err.contains("BITCLAW_TESTE_SEM_CHAVE"), "{err}");
    }
}

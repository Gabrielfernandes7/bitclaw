//! A chave de API: lida de uma variável de ambiente e impossível de vazar por `Debug` ou por erro.

use std::fmt;

pub struct ApiKey {
    value: String,
}

// Nunca imprime o valor, nem em `{:?}`.
impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(***)")
    }
}

impl ApiKey {
    // Lê a chave da variável `var`. O erro diz qual variável falta, mas não ecoa nada do valor.
    pub fn from_env(var: &str) -> Result<Self, String> {
        match std::env::var(var) {
            Ok(value) if !value.trim().is_empty() => Ok(Self::new(value)),
            Ok(_) => Err(format!("a variável de ambiente {var} está vazia: defina a chave do OpenRouter nela")),
            Err(_) => Err(format!(
                "a variável de ambiente {var} não está definida: exporte a chave do OpenRouter (export {var}=...)"
            )),
        }
    }

    pub(crate) fn new(value: String) -> Self {
        Self { value: value.trim().to_string() }
    }

    // O valor para o cabeçalho `Authorization`. É o único lugar que o expõe.
    pub(super) fn bearer(&self) -> String {
        format!("Bearer {}", self.value)
    }

    // Troca qualquer ocorrência da chave em `text` por `***`: rede de segurança para mensagens de erro.
    pub(super) fn redact(&self, text: &str) -> String {
        if self.value.is_empty() { text.to_string() } else { text.replace(&self.value, "***") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_redact_never_show_the_key() {
        let key = ApiKey::new("sk-or-segredo".into());
        assert_eq!(format!("{key:?}"), "ApiKey(***)");
        assert_eq!(key.redact("falhou com sk-or-segredo no meio"), "falhou com *** no meio");
        assert_eq!(key.bearer(), "Bearer sk-or-segredo");
    }

    #[test]
    fn a_missing_variable_is_named_in_the_error() {
        let err = ApiKey::from_env("BITCLAW_TESTE_VARIAVEL_QUE_NAO_EXISTE").unwrap_err();
        assert!(err.contains("BITCLAW_TESTE_VARIAVEL_QUE_NAO_EXISTE"), "{err}");
    }
}

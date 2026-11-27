//! A chave de API: lida de uma variável de ambiente e impossível de vazar por `Debug` ou por erro.

use std::fmt;
use std::path::Path;

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

    // Como `from_env`, e se a variável não existe lê a chave guardada em `file` (o que `bitclaw openrouter`
    // grava, só com permissão do dono). A variável sempre vale mais que o arquivo.
    pub fn from_env_or_file(var: &str, file: Option<&Path>) -> Result<Self, String> {
        if std::env::var(var).is_ok_and(|v| !v.trim().is_empty()) {
            return Self::from_env(var);
        }
        let Some(file) = file.filter(|f| f.is_file()) else {
            return Err(format!(
                "não achei a chave do OpenRouter: a variável {var} não está definida e não há chave salva. Rode `bitclaw openrouter` para configurar (ou export {var}=...)"
            ));
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = file.metadata().map_err(|e| format!("não consegui ler {}: {e}", file.display()))?.permissions().mode();
            if mode & 0o077 != 0 {
                return Err(format!("{} pode ser lido por outras pessoas: rode chmod 600 {}", file.display(), file.display()));
            }
        }
        let value = std::fs::read_to_string(file).map_err(|e| format!("não consegui ler {}: {e}", file.display()))?;
        if value.trim().is_empty() {
            return Err(format!("{} está vazio: rode `bitclaw openrouter` de novo", file.display()));
        }
        Ok(Self::new(value))
    }

    // A chave tal como foi digitada, já aparada. `None` se vazia.
    pub fn from_value(value: &str) -> Option<Self> {
        (!value.trim().is_empty()).then(|| Self::new(value.to_string()))
    }

    // O texto da chave, só para gravá-la no arquivo do dono. Nunca vai para mensagem nem log.
    pub fn expose_for_saving(&self) -> &str {
        &self.value
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

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bitclaw-auth-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn the_saved_key_is_read_only_when_just_the_owner_can_read_it() {
        use std::os::unix::fs::PermissionsExt;
        let file = temp("file").join("openrouter.key");
        std::fs::write(&file, "sk-or-guardada\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let err = ApiKey::from_env_or_file("BITCLAW_TESTE_SEM_VARIAVEL", Some(&file)).unwrap_err();
        assert!(err.contains("chmod 600") && !err.contains("guardada"), "{err}");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let key = ApiKey::from_env_or_file("BITCLAW_TESTE_SEM_VARIAVEL", Some(&file)).unwrap();
        assert_eq!(key.bearer(), "Bearer sk-or-guardada");
    }

    #[test]
    fn without_variable_or_file_the_error_points_at_the_command() {
        let err = ApiKey::from_env_or_file("BITCLAW_TESTE_SEM_VARIAVEL", None).unwrap_err();
        assert!(err.contains("bitclaw openrouter") && err.contains("BITCLAW_TESTE_SEM_VARIAVEL"), "{err}");
    }

    #[test]
    fn a_blank_pasted_key_is_not_a_key() {
        assert!(ApiKey::from_value("  \n").is_none());
        assert_eq!(ApiKey::from_value(" sk-x ").unwrap().expose_for_saving(), "sk-x");
    }

    #[test]
    fn a_missing_variable_is_named_in_the_error() {
        let err = ApiKey::from_env("BITCLAW_TESTE_VARIAVEL_QUE_NAO_EXISTE").unwrap_err();
        assert!(err.contains("BITCLAW_TESTE_VARIAVEL_QUE_NAO_EXISTE"), "{err}");
    }
}

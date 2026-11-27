//! A conta por trás da chave: `GET /auth/key` diz se a chave vale e quanto crédito resta.

use super::auth::ApiKey;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    // O nome que a pessoa deu à chave no painel do OpenRouter.
    pub label: Option<String>,
    // Gasto até agora e teto da chave, em dólares. `None` no teto = sem limite.
    pub usage: f64,
    pub limit: Option<f64>,
    pub free_tier: bool,
}

impl Account {
    // Crédito que ainda dá para gastar com esta chave, se ela tem teto.
    pub fn remaining(&self) -> Option<f64> {
        self.limit.map(|l| (l - self.usage).max(0.0))
    }
}

// Lê a resposta de `/auth/key` (`{"data": {...}}`). Campos que faltam não derrubam a leitura.
pub fn parse_account(v: &Value) -> Option<Account> {
    let d = v.get("data")?;
    Some(Account {
        label: d.get("label").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from),
        usage: d.get("usage").and_then(Value::as_f64).unwrap_or(0.0),
        limit: d.get("limit").and_then(Value::as_f64),
        free_tier: d.get("is_free_tier").and_then(Value::as_bool).unwrap_or(false),
    })
}

// Confere a chave no OpenRouter. O erro nunca repete a chave.
pub fn check_key(base_url: &str, key: &ApiKey) -> Result<Account, String> {
    let http = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = http
        .get(format!("{base_url}/auth/key"))
        .header("Authorization", key.bearer())
        .send()
        .map_err(|e| key.redact(&format!("não consegui falar com o OpenRouter ({base_url}): {e}")))?;
    match resp.status().as_u16() {
        200 => {
            let v: Value = resp.json().map_err(|e| format!("resposta ilegível do OpenRouter: {e}"))?;
            parse_account(&v).ok_or_else(|| "resposta inesperada do OpenRouter em /auth/key".to_string())
        }
        401 | 403 => Err("o OpenRouter recusou a chave (HTTP 401): ela está certa e ativa?".into()),
        n => Err(format!("o OpenRouter respondeu HTTP {n} ao conferir a chave")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_account_is_read_from_the_data_object() {
        let a = parse_account(&json!({"data": {"label": "bitclaw", "usage": 1.25, "limit": 5.0, "is_free_tier": false}})).unwrap();
        assert_eq!(a.label.as_deref(), Some("bitclaw"));
        assert_eq!(a.remaining(), Some(3.75));
        assert!(!a.free_tier);
    }

    #[test]
    fn missing_fields_are_tolerated_and_no_limit_means_no_remaining() {
        let a = parse_account(&json!({"data": {"is_free_tier": true}})).unwrap();
        let remaining = a.remaining();
        assert_eq!((a.label, a.usage, remaining, a.free_tier), (None, 0.0, None, true));
        assert!(parse_account(&json!({"erro": 1})).is_none());
    }

    #[test]
    fn an_overspent_limit_does_not_go_negative() {
        let a = parse_account(&json!({"data": {"usage": 9.0, "limit": 5.0}})).unwrap();
        assert_eq!(a.remaining(), Some(0.0));
    }
}

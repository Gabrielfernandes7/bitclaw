//! Sessão de um agente: histórico, orçamento de contexto e um turno por vez.
//! É a API pública do core. Um agente novo monta uma `Session` e chama `submit`; não precisa
//! conhecer o loop nem o adaptador de modelo.

use crate::agent::{self, Limits, Progress, TurnReport};
use crate::config::AgentConfig;
use crate::context::{self, ContextManager, Usage};
use crate::tools::{Approver, Registry};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Session {
    model: Box<dyn crate::agent::ChatModel>,
    registry: Registry,
    approver: Box<dyn Approver>,
    root: PathBuf,
    limits: Limits,
    ctx: ContextManager,
    messages: Vec<Value>,
}

impl Session {
    // Monta a sessão a partir de uma configuração. O modelo é injetado: o core não cria adaptadores.
    pub fn from_config(
        cfg: &AgentConfig,
        model: Box<dyn crate::agent::ChatModel>,
        approver: Box<dyn Approver>,
        root: &Path,
    ) -> Result<Self, String> {
        cfg.validate()?;
        let registry = Registry::from_names(&cfg.tools)?;
        let root = root.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
        let limits = Limits {
            max_steps: cfg.limits.max_steps,
            max_consecutive_failures: cfg.limits.max_consecutive_failures,
        };
        let ctx = ContextManager::new(cfg.model.num_ctx as usize, cfg.limits.reserve_output_tokens);
        let messages = vec![json!({"role": "system", "content": cfg.system_prompt})];
        Ok(Self { model, registry, approver, root, limits, ctx, messages })
    }

    // Envia uma tarefa e roda o ciclo até a resposta final (ou um limite).
    //
    // Se o turno falhar (ex.: o provedor do modelo está fora), o histórico volta ao estado anterior.
    // Assim, a tarefa que falhou não fica pendente e não é reenviada junto com a próxima.
    pub fn submit(&mut self, input: &str, progress: &dyn Progress) -> Result<TurnReport, String> {
        // Cópia do histórico: o `trim` pode remover mensagens antigas durante o turno,
        // então desfazer por índice não seria seguro.
        let snapshot = self.messages.clone();
        self.messages.push(json!({"role": "user", "content": input}));
        let result = agent::run_turn_with(
            self.model.as_ref(),
            &self.registry,
            &self.root,
            &mut self.messages,
            self.approver.as_ref(),
            &mut self.ctx,
            progress,
            self.limits,
        );
        if result.is_err() {
            self.messages = snapshot;
        }
        result
    }

    // Uso atual da janela de contexto.
    pub fn usage(&self) -> Usage {
        self.ctx.usage(&self.messages, &self.registry.schemas())
    }

    // Apaga o histórico e mantém o prompt do sistema.
    pub fn clear(&mut self) {
        context::clear(&mut self.messages);
    }

    pub fn messages(&self) -> &[Value] {
        &self.messages
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{ChatModel, Reply, Silent};
    use crate::tools::AutoApprover;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;

    struct Scripted(RefCell<VecDeque<Value>>);

    impl ChatModel for Scripted {
        fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
            let message = self.0.borrow_mut().pop_front().ok_or("roteiro esgotado")?;
            Ok(Reply { message, prompt_tokens: None })
        }
    }

    fn scripted(replies: Vec<Value>) -> Box<dyn ChatModel> {
        Box::new(Scripted(RefCell::new(replies.into())))
    }

    fn config(extra_limits: &str) -> AgentConfig {
        let text = format!(
            r#"
name = "teste"
system_prompt = "sistema de teste"
tools = ["create_file", "read_file"]

[model]
name = "m"
url = "http://unused"

[limits]
{extra_limits}
"#
        );
        AgentConfig::from_toml(&text).unwrap()
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-session-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn submit_runs_a_turn_with_the_configured_tools() {
        let dir = tempdir("submit");
        let model = scripted(vec![
            json!({"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "create_file", "arguments": {"path": "a.txt", "content": "oi"}}}
            ]}),
            json!({"role": "assistant", "content": "Pronto."}),
        ]);
        let mut s = Session::from_config(&config(""), model, Box::new(AutoApprover), &dir).unwrap();
        let report = s.submit("crie a.txt", &Silent).unwrap();
        assert!(report.changed());
        assert_eq!(fs::read_to_string(s.root().join("a.txt")).unwrap(), "oi");
    }

    #[test]
    fn configured_tool_list_is_enforced() {
        // `delete_file` não está na configuração: o modelo não pode usá-la.
        let dir = tempdir("enforced");
        fs::write(dir.join("a.txt"), "x").unwrap();
        let model = scripted(vec![
            json!({"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "delete_file", "arguments": {"path": "a.txt"}}}
            ]}),
            json!({"role": "assistant", "content": "ok"}),
        ]);
        let mut s = Session::from_config(&config(""), model, Box::new(AutoApprover), &dir).unwrap();
        let report = s.submit("apague", &Silent).unwrap();
        assert!(!report.deleted());
        assert!(dir.join("a.txt").exists());
    }

    #[test]
    fn configured_step_limit_is_applied() {
        let dir = tempdir("steps");
        for i in 0..5 {
            fs::write(dir.join(format!("n{i}.txt")), "x").unwrap();
        }
        let replies: Vec<Value> = (0..5)
            .map(|i| json!({"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "read_file", "arguments": {"path": format!("n{i}.txt")}}}
            ]}))
            .collect();
        let mut s = Session::from_config(&config("max_steps = 2"), scripted(replies), Box::new(AutoApprover), &dir).unwrap();
        let report = s.submit("leia", &Silent).unwrap();
        assert_eq!(report.actions.len(), 2);
        assert!(report.error.is_some());
    }

    // Modelo que falha na primeira chamada e responde nas seguintes.
    struct FailsOnce(RefCell<bool>);

    impl ChatModel for FailsOnce {
        fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
            let mut failed = self.0.borrow_mut();
            if !*failed {
                *failed = true;
                return Err("falha ao conectar no Ollama".into());
            }
            Ok(Reply { message: json!({"role": "assistant", "content": "ok"}), prompt_tokens: None })
        }
    }

    #[test]
    fn failed_turn_is_rolled_back_from_history() {
        let dir = tempdir("rollback");
        let mut s = Session::from_config(
            &config(""),
            Box::new(FailsOnce(RefCell::new(false))),
            Box::new(AutoApprover),
            &dir,
        )
        .unwrap();
        let before = s.messages().len();
        assert!(s.submit("tarefa que falhou", &Silent).is_err());
        assert_eq!(s.messages().len(), before, "o histórico deve voltar ao estado anterior");
        assert!(!s.messages().iter().any(|m| m["content"] == "tarefa que falhou"));

        // A próxima tarefa é enviada sozinha: a que falhou não volta junto.
        let report = s.submit("nova", &Silent).unwrap();
        assert_eq!(report.text, "ok");
        assert!(!s.messages().iter().any(|m| m["content"] == "tarefa que falhou"));
    }

    #[test]
    fn clear_keeps_only_the_system_prompt() {
        let dir = tempdir("clear");
        let mut s = Session::from_config(&config(""), scripted(vec![json!({"role": "assistant", "content": "ok"})]), Box::new(AutoApprover), &dir).unwrap();
        s.submit("oi", &Silent).unwrap();
        assert!(s.messages().len() > 1);
        s.clear();
        assert_eq!(s.messages().len(), 1);
        assert_eq!(s.messages()[0]["content"], "sistema de teste");
    }

    #[test]
    fn invalid_config_is_rejected_before_building() {
        let dir = tempdir("invalid");
        let mut cfg = config("");
        cfg.tools = vec!["nao_existe".into()];
        assert!(Session::from_config(&cfg, scripted(vec![]), Box::new(AutoApprover), &dir).is_err());
    }
}

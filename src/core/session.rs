//! Sessão de um agente: histórico, orçamento de contexto e um turno por vez.
//! É a API pública do core. Um agente novo monta uma `Session` e chama `submit`; não precisa
//! conhecer o loop nem o adaptador de modelo.

use crate::core::agent::{self, Limits, Progress, TurnReport};
use crate::core::config::AgentConfig;
use crate::core::context::{self, ContextManager, Usage};
use crate::core::skill::{self as skills, Disabled, Skill};
use std::collections::HashSet;
use std::sync::Arc;
use crate::core::tool::{Approver, Registry};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Session {
    model: Box<dyn crate::core::agent::ChatModel>,
    registry: Registry,
    approver: Box<dyn Approver>,
    root: PathBuf,
    limits: Limits,
    ctx: ContextManager,
    messages: Vec<Value>,
    // Prompt do sistema sem o catálogo de skills, e todas as skills carregadas.
    base_system: String,
    all_skills: Vec<Skill>,
    // Skills desligadas pelo usuário, e as que já estão refletidas no prompt do sistema.
    disabled: Disabled,
    applied: Option<HashSet<String>>,
}

impl Session {
    // Monta a sessão a partir de uma configuração. O modelo é injetado: o core não cria adaptadores.
    pub fn from_config(
        cfg: &AgentConfig,
        model: Box<dyn crate::core::agent::ChatModel>,
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
        let base_system = cfg.system_prompt.clone();
        Ok(Self {
            model,
            registry,
            approver,
            root,
            limits,
            ctx,
            messages,
            base_system,
            all_skills: Vec::new(),
            disabled: skills::new_disabled(),
            applied: None,
        })
    }

    // Oferece skills ao modelo: o catálogo vai ao prompt do sistema e `load_skill` é registrada.
    // Sem skills, a sessão não muda.
    pub fn with_skills(mut self, skills: Vec<Skill>, disabled: Disabled) -> Self {
        if skills.is_empty() {
            return self;
        }
        self.registry = self.registry.with_tool(Box::new(skills::LoadSkill::new(skills.clone(), Arc::clone(&disabled))));
        self.all_skills = skills;
        self.disabled = disabled;
        self.apply_skills();
        self
    }

    // Reconstrói o prompt do sistema com o catálogo das skills ligadas, se a escolha mudou.
    fn apply_skills(&mut self) {
        let off = skills::disabled_now(&self.disabled);
        if self.applied.as_ref() == Some(&off) {
            return;
        }
        let enabled: Vec<Skill> = self.all_skills.iter().filter(|s| !off.contains(&s.name)).cloned().collect();
        let catalog = skills::catalog(&enabled);
        let content = if catalog.is_empty() {
            self.base_system.clone()
        } else {
            format!("{}\n\n{catalog}", self.base_system)
        };
        if let Some(system) = self.messages.first_mut() {
            system["content"] = Value::String(content);
        }
        self.applied = Some(off);
    }

    // Anexa o corpo de cada skill ligada cujo nome aparece na tarefa. Não depende de o modelo chamar `load_skill`.
    fn with_named_skills(&self, input: &str) -> String {
        let lower = input.to_lowercase();
        let mut text = input.to_string();
        let off = skills::disabled_now(&self.disabled);
        for skill in self.all_skills.iter().filter(|s| !off.contains(&s.name)) {
            if lower.contains(&skill.name.to_lowercase()) {
                text.push_str(&format!("\n\n[Instruções da skill {}]\n{}", skill.name, skill.body));
            }
        }
        text
    }

    // Envia uma tarefa e roda o ciclo até a resposta final (ou um limite).
    //
    // Se o turno falhar (ex.: o provedor do modelo está fora), o histórico volta ao estado anterior.
    // Assim, a tarefa que falhou não fica pendente e não é reenviada junto com a próxima.
    pub fn submit(&mut self, input: &str, progress: &dyn Progress) -> Result<TurnReport, String> {
        self.apply_skills();
        // Cópia do histórico: o `trim` pode remover mensagens antigas durante o turno,
        // então desfazer por índice não seria seguro.
        let snapshot = self.messages.clone();
        // O raciocínio de turnos anteriores não volta ao modelo: só gastaria contexto. O deste turno
        // fica no histórico, para o modelo continuar o raciocínio entre uma ferramenta e outra.
        for message in &mut self.messages {
            if let Some(object) = message.as_object_mut() {
                object.remove("thinking");
            }
        }
        let input = self.with_named_skills(input);
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

    // `/init`: cria o AGENTS.md do projeto, depois de aprovação. Não sobrescreve um arquivo existente.
    pub fn init_agents(&self) -> Result<String, String> {
        crate::init::write(&self.root, self.approver.as_ref())
    }

    // Troca o modelo no meio da conversa. O histórico é mensagens neutras de provedor, então segue
    // com a sessão; a próxima tarefa vai para o novo modelo.
    pub fn set_model(&mut self, model: Box<dyn crate::core::agent::ChatModel>) {
        self.model = model;
    }

    // Troca a janela de contexto. O histórico não muda; só o orçamento a partir do próximo turno.
    pub fn set_context_window(&mut self, num_ctx: usize) -> Result<(), String> {
        self.ctx.set_num_ctx(num_ctx)
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
    use crate::core::agent::{ChatModel, Reply, Silent};
    use crate::core::tool::AutoApprover;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::rc::Rc;

    struct Scripted(RefCell<VecDeque<Value>>);

    impl ChatModel for Scripted {
        fn complete(&self, _m: &[Value], _t: &Value) -> Result<Reply, String> {
            let message = self.0.borrow_mut().pop_front().ok_or("roteiro esgotado")?;
            Ok(Reply { message, prompt_tokens: None, output_tokens: None })
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
    fn skills_join_the_prompt_as_catalog_and_load_skill_serves_the_body() {
        let dir = tempdir("skills");
        let skill_dir = dir.join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: demo\ndescription: faz demo\n---\n\nPasso secreto.\n").unwrap();
        let skills = crate::core::skill::load(&dir.join("skills")).unwrap();
        let model = scripted(vec![
            json!({"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "load_skill", "arguments": {"name": "demo"}}}
            ]}),
            json!({"role": "assistant", "content": "Li a skill."}),
        ]);
        let mut s = Session::from_config(&config(""), model, Box::new(AutoApprover), &dir)
            .unwrap()
            .with_skills(skills, skills::new_disabled());
        let system = s.messages[0]["content"].as_str().unwrap().to_string();
        assert!(system.starts_with("sistema de teste"));
        assert!(system.contains("- demo: faz demo"));
        assert!(!system.contains("Passo secreto"), "o corpo não entra no prompt");
        let report = s.submit("use demo", &Silent).unwrap();
        assert!(report.text.contains("Li a skill"));
        assert!(s.messages.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("Passo secreto"))));
    }

    #[test]
    fn a_task_naming_a_skill_gets_its_body_without_load_skill() {
        let dir = tempdir("named");
        let skill_dir = dir.join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: demo\ndescription: faz demo\n---\n\nPasso secreto.\n").unwrap();
        let skills = crate::core::skill::load(&dir.join("skills")).unwrap();
        let model = scripted(vec![json!({"role": "assistant", "content": "Feito."})]);
        let mut s = Session::from_config(&config(""), model, Box::new(AutoApprover), &dir)
            .unwrap()
            .with_skills(skills, skills::new_disabled());
        s.submit("use a skill DEMO nesta tarefa", &Silent).unwrap();
        let user = s.messages.iter().find(|m| m["role"] == "user").unwrap()["content"].as_str().unwrap().to_string();
        assert!(user.contains("Passo secreto"), "o corpo da skill entra no turno");
    }

    #[test]
    fn disabled_skill_leaves_the_prompt_and_cannot_be_loaded() {
        let dir = tempdir("disabled");
        let skill_dir = dir.join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: demo\ndescription: faz demo\n---\n\nPasso secreto.\n").unwrap();
        let skills = crate::core::skill::load(&dir.join("skills")).unwrap();
        let off = crate::core::skill::new_disabled();
        let mut s = Session::from_config(&config(""), scripted(vec![]), Box::new(AutoApprover), &dir)
            .unwrap()
            .with_skills(skills, Arc::clone(&off));
        assert!(s.messages[0]["content"].as_str().unwrap().contains("- demo:"));
        off.lock().unwrap().insert("demo".into());
        s.apply_skills();
        let system = s.messages[0]["content"].as_str().unwrap();
        assert!(!system.contains("demo"), "a skill desligada sai do catálogo");
        s.submit("use demo", &Silent).ok();
        assert!(!s.messages.iter().any(|m| m["content"].as_str().is_some_and(|c| c.contains("Passo secreto"))));
        off.lock().unwrap().clear();
        s.apply_skills();
        assert!(s.messages[0]["content"].as_str().unwrap().contains("- demo:"), "ligar de novo volta ao catálogo");
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

    // Modelo que anota o tamanho do histórico que recebe, para provar que a conversa seguiu com ele.
    struct Recording {
        replies: RefCell<VecDeque<Value>>,
        seen: Rc<RefCell<Vec<Vec<String>>>>,
    }

    impl ChatModel for Recording {
        fn complete(&self, messages: &[Value], _t: &Value) -> Result<Reply, String> {
            let roles: Vec<String> = messages
                .iter()
                .map(|m| m.get("content").and_then(Value::as_str).unwrap_or("").to_string())
                .collect();
            self.seen.borrow_mut().push(roles);
            let message = self.replies.borrow_mut().pop_front().ok_or("roteiro esgotado")?;
            Ok(Reply { message, prompt_tokens: None, output_tokens: None })
        }
    }

    #[test]
    fn switching_model_mid_conversation_keeps_history_and_tools() {
        let dir = tempdir("switch");
        let first = scripted(vec![json!({"role": "assistant", "content": "primeira resposta"})]);
        let mut s = Session::from_config(&config(""), first, Box::new(AutoApprover), &dir).unwrap();
        s.submit("primeira tarefa", &Silent).unwrap();

        let seen = Rc::new(RefCell::new(Vec::new()));
        s.set_model(Box::new(Recording {
            replies: RefCell::new(
                vec![
                    json!({"role": "assistant", "content": "", "tool_calls": [
                        {"function": {"name": "create_file", "arguments": {"path": "b.txt", "content": "depois"}}}
                    ]}),
                    json!({"role": "assistant", "content": "Pronto."}),
                ]
                .into(),
            ),
            seen: Rc::clone(&seen),
        }));
        let report = s.submit("segunda tarefa", &Silent).unwrap();

        // O novo modelo recebeu a primeira troca da conversa antes da nova tarefa.
        let first_call = &seen.borrow()[0];
        assert!(first_call.contains(&"primeira tarefa".to_string()), "{first_call:?}");
        assert!(first_call.contains(&"primeira resposta".to_string()), "{first_call:?}");
        // E as ferramentas continuam funcionando depois da troca.
        assert!(report.changed());
        assert_eq!(fs::read_to_string(s.root().join("b.txt")).unwrap(), "depois");
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
            Ok(Reply { message: json!({"role": "assistant", "content": "ok"}), prompt_tokens: None, output_tokens: None })
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
    fn thinking_from_earlier_turns_is_dropped_but_the_answer_stays() {
        let dir = tempdir("thinking");
        let model = scripted(vec![
            json!({"role": "assistant", "content": "primeira", "thinking": "raciocínio longo do primeiro turno"}),
            json!({"role": "assistant", "content": "segunda", "thinking": "raciocínio do segundo turno"}),
        ]);
        let mut s = Session::from_config(&config(""), model, Box::new(AutoApprover), &dir).unwrap();
        s.submit("um", &Silent).unwrap();
        assert!(s.messages().iter().any(|m| m.get("thinking").is_some()), "o do turno atual fica");
        s.submit("dois", &Silent).unwrap();
        let thinking: Vec<_> = s.messages().iter().filter_map(|m| m.get("thinking")).collect();
        assert_eq!(thinking.len(), 1, "só o do turno mais recente");
        assert_eq!(thinking[0], "raciocínio do segundo turno");
        assert!(s.messages().iter().any(|m| m["content"] == "primeira"));
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

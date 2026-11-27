//! Sessão de um agente: histórico, orçamento de contexto e um turno por vez.
//! É a API pública do core. Um agente novo monta uma `Session` e chama `submit`; não precisa
//! conhecer o loop nem o adaptador de modelo.

use crate::core::agent::{self, ChatModel, Limits, Progress, TurnHook, TurnReport, Verifier};
use crate::core::config::AgentConfig;
use crate::core::context::{self, ContextManager, Usage};
use crate::core::retrieve::Retriever;
use crate::core::skill::{self as skills, Disabled, Skill};
use std::collections::HashSet;
use std::sync::Arc;
use crate::core::tool::{Approver, Registry, specs_value};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Session {
    model: Box<dyn ChatModel>,
    registry: Registry,
    approver: Box<dyn Approver>,
    hooks: Vec<Box<dyn TurnHook>>,
    retriever: Option<Box<dyn Retriever>>,
    verifier: Option<Box<dyn Verifier>>,
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

// Monta uma `Session`. O modelo, o aprovador e as ferramentas são injetados: o core não cria
// adaptadores nem escolhe ferramentas.
//
// ```ignore
// let session = Session::builder(&cfg)
//     .model(model)
//     .approver(approver)
//     .tools(registry) // toolkit::registry_from_names(&cfg.tools)? + as ferramentas do fork
//     .root(path)
//     .build()?;
// ```
pub struct SessionBuilder<'a> {
    cfg: &'a AgentConfig,
    model: Option<Box<dyn ChatModel>>,
    approver: Option<Box<dyn Approver>>,
    tools: Option<Registry>,
    hooks: Vec<Box<dyn TurnHook>>,
    retriever: Option<Box<dyn Retriever>>,
    verifier: Option<Box<dyn Verifier>>,
    persona: Option<String>,
    root: PathBuf,
}

// Teto, em caracteres, da persona (SOUL.md) que entra no prompt do sistema. Num modelo pequeno, cada
// token do prompt tira espaço da conversa; o que passa disto é cortado.
pub const MAX_PERSONA_CHARS: usize = 1_200;
// Título da persona no prompt. Diz que ela já está aqui: sem isso, o modelo pequeno procura o SOUL.md
// no projeto com as ferramentas (ele fica fora da pasta do projeto) e não responde sobre a pessoa.
const PERSONA_HEADER: &str = "SOBRE A PESSOA E O SOUL.md (já está aqui, não use ferramenta para consultar; responda perguntas sobre a pessoa com isto):";

impl<'a> SessionBuilder<'a> {
    // Quem o agente é e como fala (o conteúdo do SOUL.md). Vai depois do prompt da configuração,
    // cortado em `MAX_PERSONA_CHARS`. Texto vazio não muda nada.
    pub fn persona(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        let text = text.trim();
        self.persona = (!text.is_empty()).then(|| text.chars().take(MAX_PERSONA_CHARS).collect());
        self
    }

    pub fn model(mut self, model: Box<dyn ChatModel>) -> Self {
        self.model = Some(model);
        self
    }

    pub fn approver(mut self, approver: Box<dyn Approver>) -> Self {
        self.approver = Some(approver);
        self
    }

    pub fn tools(mut self, tools: Registry) -> Self {
        self.tools = Some(tools);
        self
    }

    // Liga um gancho que roda depois de cada turno e pode acrescentar avisos ao relatório.
    pub fn hook(mut self, hook: Box<dyn TurnHook>) -> Self {
        self.hooks.push(hook);
        self
    }

    // Anexa a cada tarefa os trechos do projeto que o `Retriever` achar prováveis.
    pub fn retriever(mut self, retriever: Box<dyn Retriever>) -> Self {
        self.retriever = Some(retriever);
        self
    }

    // Depois de uma alteração, roda a verificação e devolve o problema ao modelo para corrigir.
    pub fn verifier(mut self, verifier: Box<dyn Verifier>) -> Self {
        self.verifier = Some(verifier);
        self
    }

    // Pasta do projeto. Sem esta chamada, a pasta atual.
    pub fn root(mut self, root: impl Into<PathBuf>) -> Self {
        self.root = root.into();
        self
    }

    pub fn build(self) -> Result<Session, String> {
        let cfg = self.cfg;
        cfg.validate()?;
        let model = self.model.ok_or("sessão sem modelo: chame .model(...)")?;
        let approver = self.approver.ok_or("sessão sem aprovador: chame .approver(...)")?;
        let registry = self.tools.ok_or("sessão sem ferramentas: chame .tools(...)")?;
        let root = self.root.canonicalize().map_err(|e| format!("diretório inválido: {e}"))?;
        let limits = Limits {
            max_steps: cfg.limits.max_steps,
            max_consecutive_failures: cfg.limits.max_consecutive_failures,
            phased: cfg.limits.phased,
            max_tool_output: cfg.limits.max_tool_output,
        };
        let ctx = ContextManager::new(cfg.model.context_window as usize, cfg.limits.reserve_output_tokens);
        let base_system = match &self.persona {
            Some(persona) => format!("{}\n\n{PERSONA_HEADER}\n{persona}", cfg.system_prompt),
            None => cfg.system_prompt.clone(),
        };
        let messages = vec![json!({"role": "system", "content": base_system})];
        Ok(Session {
            model,
            registry,
            approver,
            hooks: self.hooks,
            retriever: self.retriever,
            verifier: self.verifier,
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
}

impl Session {
    pub fn builder(cfg: &AgentConfig) -> SessionBuilder<'_> {
        SessionBuilder { cfg, model: None, approver: None, tools: None, hooks: Vec::new(), retriever: None, verifier: None, persona: None, root: PathBuf::from(".") }
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
                // O corpo cita os arquivos da skill como caminhos (references/x.md). Sem este aviso, um
                // modelo pequeno os procura no projeto com read_file, onde eles não estão.
                if !skill.files.is_empty() {
                    text.push_str(&format!(
                        "\n[Arquivos da skill {0}: {1}. Não estão no projeto: leia com load_skill, name \"{0}\" e file.]",
                        skill.name,
                        skill.files.join(", ")
                    ));
                }
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
        let mut input = self.with_named_skills(input);
        if let Some(found) = self.retriever.as_ref().and_then(|r| r.retrieve(&self.root, &input)) {
            input.push_str(&format!("\n\n{found}"));
        }
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
            self.verifier.as_deref(),
        );
        if result.is_err() {
            self.messages = snapshot;
        }
        result.map(|mut report| {
            for hook in &self.hooks {
                report.warnings.extend(hook.after_turn(&report));
            }
            report
        })
    }

    // Quem aprova as alterações desta sessão (ex.: o `/init` do app a reaproveita).
    pub fn approver(&self) -> &dyn Approver {
        self.approver.as_ref()
    }

    // Troca o modelo no meio da conversa. O histórico é mensagens neutras de provedor, então segue
    // com a sessão; a próxima tarefa vai para o novo modelo.
    pub fn set_model(&mut self, model: Box<dyn ChatModel>) {
        self.model = model;
    }

    // Troca a janela de contexto. O histórico não muda; só o orçamento a partir do próximo turno.
    pub fn set_context_window(&mut self, num_ctx: usize) -> Result<(), String> {
        self.ctx.set_num_ctx(num_ctx)
    }

    // Uso atual da janela de contexto.
    pub fn usage(&self) -> Usage {
        self.ctx.usage(&self.messages, &specs_value(&self.registry.specs()))
    }

    // Apaga o histórico e mantém o prompt do sistema.
    pub fn clear(&mut self) {
        context::clear(&mut self.messages);
    }

    // Liga ou desliga o modo Plan (só ferramentas de leitura) no meio da sessão.
    pub fn set_plan(&mut self, on: bool) {
        self.registry.set_plan(on);
    }

    pub fn plan(&self) -> bool {
        self.registry.plan()
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
    use crate::core::tool::{AutoApprover, ToolSpec};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::fs;
    use std::rc::Rc;

    struct Scripted(RefCell<VecDeque<Value>>);

    impl ChatModel for Scripted {
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
            let message = self.0.borrow_mut().pop_front().ok_or("roteiro esgotado")?;
            Ok(Reply { message, prompt_tokens: None, output_tokens: None })
        }
    }

    fn scripted(replies: Vec<Value>) -> Box<dyn ChatModel> {
        Box::new(Scripted(RefCell::new(replies.into())))
    }

    // Sessão de teste: ferramentas do toolkit pelos nomes da configuração, aprovação automática.
    fn from_parts(cfg: &AgentConfig, model: Box<dyn ChatModel>, root: &Path) -> Result<Session, String> {
        let tools = crate::toolkit::registry_from_names(&cfg.tools)?;
        Session::builder(cfg).model(model).approver(Box::new(AutoApprover)).tools(tools).root(root).build()
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
    fn plan_mode_toggles_at_runtime_without_losing_tools() {
        let dir = tempdir("plan-toggle");
        let mut s = from_parts(&config(""), scripted(vec![]), &dir).unwrap();
        let all = s.registry.names();
        assert!(all.contains(&"create_file") && all.contains(&"read_file"));
        s.set_plan(true);
        assert!(s.plan());
        assert_eq!(s.registry.names(), vec!["read_file"]);
        let refused = s.registry.execute(&crate::core::tool::ToolContext::new(&dir, &AutoApprover), "create_file", &json!({"path": "x.txt", "content": "x"}));
        assert!(matches!(refused, crate::core::tool::Outcome::Error(ref e) if e.contains("modo Plan")), "{refused:?}");
        assert!(!dir.join("x.txt").exists());
        s.set_plan(false);
        assert_eq!(s.registry.names(), all);
    }

    #[test]
    fn persona_follows_the_system_prompt_and_is_capped() {
        let dir = tempdir("persona");
        let cfg = config("");
        let tools = crate::toolkit::registry_from_names(&cfg.tools).unwrap();
        let s = Session::builder(&cfg)
            .model(scripted(vec![]))
            .approver(Box::new(AutoApprover))
            .tools(tools)
            .root(&dir)
            .persona(format!("  Chame de Ana.\n{}", "x".repeat(MAX_PERSONA_CHARS * 2)))
            .build()
            .unwrap();
        let system = s.messages()[0]["content"].as_str().unwrap();
        assert!(system.starts_with("sistema de teste\n\nSOBRE A PESSOA"));
        assert!(system.contains("\nChame de Ana."));
        assert!(system.chars().count() <= "sistema de teste\n\n".len() + PERSONA_HEADER.chars().count() + 1 + MAX_PERSONA_CHARS);
        // Texto vazio não muda o prompt.
        let tools = crate::toolkit::registry_from_names(&cfg.tools).unwrap();
        let s = Session::builder(&cfg).model(scripted(vec![])).approver(Box::new(AutoApprover)).tools(tools).root(&dir).persona("  ").build().unwrap();
        assert_eq!(s.messages()[0]["content"], "sistema de teste");
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
        let mut s = from_parts(&config(""), model, &dir)
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
        let mut s = from_parts(&config(""), model, &dir)
            .unwrap()
            .with_skills(skills, skills::new_disabled());
        s.submit("use a skill DEMO nesta tarefa", &Silent).unwrap();
        let user = s.messages.iter().find(|m| m["role"] == "user").unwrap()["content"].as_str().unwrap().to_string();
        assert!(user.contains("Passo secreto"), "o corpo da skill entra no turno");
    }

    #[test]
    fn a_named_skill_with_files_says_they_are_read_with_load_skill() {
        let dir = tempdir("named-files");
        let skill_dir = dir.join("skills/demo");
        fs::create_dir_all(skill_dir.join("references")).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: demo\ndescription: faz demo\n---\n\nLeia references/x.md.\n").unwrap();
        fs::write(skill_dir.join("references/x.md"), "detalhe").unwrap();
        let skills = crate::core::skill::load(&dir.join("skills")).unwrap();
        let model = scripted(vec![json!({"role": "assistant", "content": "Feito."})]);
        let mut s = from_parts(&config(""), model, &dir).unwrap().with_skills(skills, skills::new_disabled());
        s.submit("use a skill demo", &Silent).unwrap();
        let user = s.messages.iter().find(|m| m["role"] == "user").unwrap()["content"].as_str().unwrap().to_string();
        assert!(user.contains("references/x.md. Não estão no projeto: leia com load_skill, name \"demo\" e file"), "{user}");
    }

    #[test]
    fn disabled_skill_leaves_the_prompt_and_cannot_be_loaded() {
        let dir = tempdir("disabled");
        let skill_dir = dir.join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "---\nname: demo\ndescription: faz demo\n---\n\nPasso secreto.\n").unwrap();
        let skills = crate::core::skill::load(&dir.join("skills")).unwrap();
        let off = crate::core::skill::new_disabled();
        let mut s = from_parts(&config(""), scripted(vec![]), &dir)
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
        let mut s = from_parts(&config(""), model, &dir).unwrap();
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
        fn complete(&self, messages: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
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
        let mut s = from_parts(&config(""), first, &dir).unwrap();
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
        let mut s = from_parts(&config(""), model, &dir).unwrap();
        let report = s.submit("apague", &Silent).unwrap();
        assert!(!crate::toolkit::claims::deleted(&report));
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
        let mut s = from_parts(&config("max_steps = 2"), scripted(replies), &dir).unwrap();
        let report = s.submit("leia", &Silent).unwrap();
        assert_eq!(report.actions.len(), 2);
        assert!(report.error.is_some());
    }

    // Modelo que falha na primeira chamada e responde nas seguintes.
    struct FailsOnce(RefCell<bool>);

    impl ChatModel for FailsOnce {
        fn complete(&self, _m: &[Value], _t: &[ToolSpec]) -> Result<Reply, String> {
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
        let mut s = from_parts(&config(""), Box::new(FailsOnce(RefCell::new(false))), &dir).unwrap();
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
        let mut s = from_parts(&config(""), model, &dir).unwrap();
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
        let mut s = from_parts(&config(""), scripted(vec![json!({"role": "assistant", "content": "ok"})]), &dir).unwrap();
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
        assert!(from_parts(&cfg, scripted(vec![]), &dir).is_err());
    }

    // O critério de sucesso: um agente novo traz a própria ferramenta, sem editar o core.
    struct Shout;

    impl crate::core::tool::Tool for Shout {
        fn name(&self) -> &'static str {
            "shout"
        }

        fn spec(&self) -> ToolSpec {
            ToolSpec::new("shout", "grita o texto", json!({"type": "object", "properties": {}}))
        }

        fn run(&self, _ctx: &crate::core::tool::ToolContext, args: &Value) -> crate::core::tool::Outcome {
            crate::core::tool::Outcome::Done(args["text"].as_str().unwrap_or("").to_uppercase())
        }
    }

    #[test]
    fn a_custom_tool_runs_without_touching_the_core() {
        let dir = tempdir("custom-tool");
        let model = scripted(vec![
            json!({"role": "assistant", "content": "", "tool_calls": [
                {"function": {"name": "shout", "arguments": {"text": "oi"}}}
            ]}),
            json!({"role": "assistant", "content": "feito"}),
        ]);
        let tools = crate::toolkit::registry_from_names(&["read_file"]).unwrap().with_tool(Box::new(Shout));
        let mut s = Session::builder(&config(""))
            .model(model)
            .approver(Box::new(AutoApprover))
            .tools(tools)
            .root(&dir)
            .build()
            .unwrap();
        let report = s.submit("grite", &Silent).unwrap();
        assert_eq!(report.actions[0].outcome, crate::core::tool::Outcome::Done("OI".into()));
    }

    #[test]
    fn the_builder_names_what_is_missing() {
        let err = Session::builder(&config("")).build().err().unwrap();
        assert!(err.contains("modelo"), "{err}");
        let err = Session::builder(&config("")).model(scripted(vec![])).approver(Box::new(AutoApprover)).build().err().unwrap();
        assert!(err.contains("ferramentas"), "{err}");
    }

    struct Flag;

    impl TurnHook for Flag {
        fn after_turn(&self, report: &TurnReport) -> Vec<String> {
            vec![format!("gancho viu: {}", report.text)]
        }
    }

    #[test]
    fn hook_warnings_land_in_the_report() {
        let dir = tempdir("hook");
        let model = scripted(vec![json!({"role": "assistant", "content": "oi"})]);
        let tools = crate::toolkit::registry_from_names(&["read_file"]).unwrap();
        let mut s = Session::builder(&config(""))
            .model(model)
            .approver(Box::new(AutoApprover))
            .tools(tools)
            .hook(Box::new(Flag))
            .root(&dir)
            .build()
            .unwrap();
        let report = s.submit("x", &Silent).unwrap();
        assert_eq!(report.warnings, vec!["gancho viu: oi"]);
    }
}

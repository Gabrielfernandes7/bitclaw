//! Ponte entre a interface e o agente: a sessão roda numa thread, e os dois conversam por canais.

use super::steps::{self, StepResult};
use super::{ModelFactory, locked};
use bitclaw::agent::{Chunk, Progress, TurnReport};
use bitclaw::context::Usage;
use bitclaw::models::{ModelInfo, SharedEffort};
use bitclaw::ollama::SharedOptions;
use bitclaw::session::Session;
use bitclaw::tools::{Approver, Outcome, Preview};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};


pub(super) enum Command {
    Submit(String),
    Clear,
    SwitchModel(ModelInfo),
    // Nova janela de contexto, em tokens.
    SetContext(usize),
    Init,
}

pub(super) enum UiEvent {
    Activity(String),
    // Um passo do agente terminou: vai para a conversa.
    Step { name: String, args: Value, result: StepResult },
    // Pedido de decisão. `runs` é verdadeiro quando a pergunta é sobre executar um script.
    Approval { preview: Preview, reply: Sender<bool>, runs: bool },
    Done { result: Result<TurnReport, String>, usage: Usage },
    Usage(Usage),
    // Tokens de uma chamada ao modelo: entrada e saída.
    Tokens { prompt: Option<u64>, output: Option<u64> },
    // Raciocínio de uma chamada ao modelo, mostrado na conversa antes da resposta.
    Reasoning(String),
    // Um pedaço de texto que o modelo está gerando agora (raciocínio ou resposta).
    Chunk { thinking: bool, text: String },
    Fatal(String),
    // Aviso curto para o histórico (troca de modelo, esforço ajustado, erro de troca).
    Notice(String),
    // Fim do `/init`: o texto de resultado ou o erro.
    InitDone(Result<String, String>),
}

// Sinais do agente viram eventos para a interface.
pub(super) struct ChannelProgress {
    pub(super) tx: Sender<UiEvent>,
    pub(super) cancel: Arc<AtomicBool>,
}

impl Progress for ChannelProgress {
    fn model_waiting(&self, waiting: bool) {
        let activity = if waiting { "pensando" } else { "respondendo" };
        let _ = self.tx.send(UiEvent::Activity(activity.into()));
    }

    fn tool_call(&self, name: &str, args: &Value) {
        let _ = self.tx.send(UiEvent::Activity(steps::doing(name, args)));
    }

    fn tool_done(&self, name: &str, args: &Value, outcome: &Outcome) {
        let result = StepResult::from_tool(name, outcome);
        let _ = self.tx.send(UiEvent::Step { name: name.to_string(), args: args.clone(), result });
    }

    fn model_tokens(&self, prompt: Option<u64>, output: Option<u64>) {
        let _ = self.tx.send(UiEvent::Tokens { prompt, output });
    }

    fn model_chunk(&self, chunk: Chunk) {
        let (thinking, text) = match chunk {
            Chunk::Thinking(t) => (true, t),
            Chunk::Content(t) => (false, t),
            // Só serve para conferir o cancelamento; não há texto para mostrar.
            Chunk::Idle => return,
        };
        let _ = self.tx.send(UiEvent::Chunk { thinking, text: text.to_string() });
    }

    fn model_reasoning(&self, text: &str) {
        let _ = self.tx.send(UiEvent::Reasoning(text.to_string()));
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

// Pede a decisão à interface e espera a resposta. Se a interface sumir, recusa.
// Com `auto_edits` (`--yes`), as edições passam sem perguntar; a execução de scripts nunca.
pub(super) struct ChannelApprover {
    pub(super) tx: Sender<UiEvent>,
    pub(super) auto_edits: bool,
}

impl ChannelApprover {
    fn ask(&self, preview: &Preview, runs: bool) -> bool {
        let (reply, answer) = mpsc::channel();
        if self.tx.send(UiEvent::Approval { preview: preview.clone(), reply, runs }).is_err() {
            return false;
        }
        answer.recv().unwrap_or(false)
    }
}

impl Approver for ChannelApprover {
    fn approve(&self, preview: &Preview) -> bool {
        self.auto_edits || self.ask(preview, false)
    }

    fn approve_run(&self, preview: &Preview) -> bool {
        self.ask(preview, true)
    }
}

// A sessão é criada dentro da thread: ela não precisa ser `Send`.
pub(super) fn worker<F>(
    make: F,
    make_model: ModelFactory,
    auto_yes: bool,
    commands: Receiver<Command>,
    tx: Sender<UiEvent>,
    cancel: Arc<AtomicBool>,
    active: Arc<Mutex<ModelInfo>>,
    effort: SharedEffort,
    options: SharedOptions,
) where
    F: FnOnce(Box<dyn Approver>) -> Result<Session, String>,
{
    let approver: Box<dyn Approver> = Box::new(ChannelApprover { tx: tx.clone(), auto_edits: auto_yes });
    let mut session = match make(approver) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send(UiEvent::Fatal(e));
            return;
        }
    };
    let _ = tx.send(UiEvent::Usage(session.usage()));
    let progress = ChannelProgress { tx: tx.clone(), cancel: Arc::clone(&cancel) };
    while let Ok(command) = commands.recv() {
        match command {
            Command::Submit(text) => {
                cancel.store(false, Ordering::Relaxed);
                let result = session.submit(&text, &progress);
                let _ = tx.send(UiEvent::Done { result, usage: session.usage() });
            }
            Command::Clear => {
                session.clear();
                let _ = tx.send(UiEvent::Usage(session.usage()));
            }
            Command::SetContext(tokens) => {
                let notice = match session.set_context_window(tokens) {
                    Ok(()) => {
                        locked(&options).num_ctx = tokens as u32;
                        let _ = tx.send(UiEvent::Usage(session.usage()));
                        format!("contexto: {tokens} tokens")
                    }
                    Err(e) => e,
                };
                let _ = tx.send(UiEvent::Notice(notice));
            }
            Command::Init => {
                let result = session.init_agents();
                let _ = tx.send(UiEvent::InitDone(result));
            }
            Command::SwitchModel(model) => {
                let notice = match make_model(&model) {
                    Ok(client) => {
                        session.set_model(client);
                        // O esforço que não existe no modelo novo vai para o nível mais próximo que existe.
                        let before = *locked(&effort);
                        let after = model.capabilities.fit(before);
                        *locked(&effort) = after;
                        let mut text = format!("modelo trocado para {}", model.id);
                        if !model.capabilities.tool_calling {
                            text.push_str(" · sem ferramentas: o agente só conversa");
                        }
                        if after != before {
                            text.push_str(&format!(" · esforço ajustado para {}", after.as_str()));
                        }
                        *locked(&active) = model;
                        text
                    }
                    Err(e) => format!("não consegui trocar para {}: {e}", model.id),
                };
                let _ = tx.send(UiEvent::Notice(notice));
            }
        }
    }
}

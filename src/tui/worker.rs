//! Ponte entre a interface e o agente: a sessão roda numa thread, e os dois conversam por canais.

use super::{ModelFactory, locked};
use bitclaw::agent::{Progress, TurnReport};
use bitclaw::context::Usage;
use bitclaw::models::{ModelInfo, SharedEffort};
use bitclaw::session::Session;
use bitclaw::tools::{Approver, AutoApprover, Preview};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};


pub(super) enum Command {
    Submit(String),
    Clear,
    SwitchModel(ModelInfo),
    Init,
}

pub(super) enum UiEvent {
    Activity(String),
    Approval { preview: Preview, reply: Sender<bool> },
    Done { result: Result<TurnReport, String>, usage: Usage },
    Usage(Usage),
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
        if waiting {
            let _ = self.tx.send(UiEvent::Activity("pensando".into()));
        }
    }

    fn tool_call(&self, name: &str, args: &Value) {
        let target = args.get("path").and_then(Value::as_str).unwrap_or("");
        let _ = self.tx.send(UiEvent::Activity(format!("executando {name} {target}").trim().to_string()));
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

// Pede a decisão à interface e espera a resposta. Se a interface sumir, recusa.
pub(super) struct ChannelApprover {
    pub(super) tx: Sender<UiEvent>,
}

impl Approver for ChannelApprover {
    fn approve(&self, preview: &Preview) -> bool {
        let (reply, answer) = mpsc::channel();
        if self.tx.send(UiEvent::Approval { preview: preview.clone(), reply }).is_err() {
            return false;
        }
        answer.recv().unwrap_or(false)
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
) where
    F: FnOnce(Box<dyn Approver>) -> Result<Session, String>,
{
    let approver: Box<dyn Approver> =
        if auto_yes { Box::new(AutoApprover) } else { Box::new(ChannelApprover { tx: tx.clone() }) };
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
                        let mut text = format!("modelo: {}", model.id);
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

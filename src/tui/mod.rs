//! Interface de terminal do BitClaw, com ratatui em viewport inline.
//!
//! O cabeçalho e a transcrição entram no scrollback normal do terminal (rolar, selecionar e
//! copiar funcionam como sempre). Só uma área fixa fica embaixo: status, editor e rodapé.
//!
//! O agente roda numa thread própria e fala com a interface por canais. Só este módulo escreve
//! na tela: a biblioteca continua sem imprimir nada.

mod brand;
mod commands;
mod history;
mod print;
mod markdown;
pub mod plain;
mod theme;
mod view;
mod worker;

use self::commands::{ARG_COMMANDS, COMMANDS, Cmd};
use self::history::History;
use self::view::*;
use self::worker::{Command, UiEvent, worker};
use self::theme::{GRAY, RED, YELLOW, bold, fg, palette};
use bitclaw::agent::ChatModel;
use bitclaw::models::{Effort, ModelCatalog, ModelInfo, Reasoning, SharedEffort};
use bitclaw::context::Usage;
use bitclaw::session::Session;
use bitclaw::skills::{Disabled, Skill};
use bitclaw::tools::Approver;
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::cursor::MoveTo;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::backend::CrosstermBackend;
use ratatui::{DefaultTerminal, Terminal, TerminalOptions, Viewport};
use ratatui_textarea::TextArea;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

// A área fixa cresce e encolhe: status (1) + régua, editor e régua (2 + linhas do editor)
// + menu de comandos + rodapé (1).
const MAX_EDITOR_ROWS: usize = 6;

fn viewport_height(editor_rows: usize, menu_rows: usize) -> u16 {
    (1 + 2 + editor_rows.clamp(1, MAX_EDITOR_ROWS) + menu_rows + 1) as u16
}

pub struct Settings {
    pub title: String,
    // Linhas ao lado do mascote, abaixo do nome.
    pub info: Vec<String>,
    // Skills carregadas, para o painel `/skills`.
    pub skills: Vec<Skill>,
    // Nomes das skills desligadas no painel. Compartilhado com a sessão do agente.
    pub skills_off: Disabled,
    // Modelo ativo (troca com `/model`), o padrão da configuração e o catálogo do provedor.
    pub model: Arc<Mutex<ModelInfo>>,
    pub default_model: String,
    pub catalog: Arc<dyn ModelCatalog + Send + Sync>,
    // Esforço de raciocínio. Compartilhado com o cliente: `/effort` e `/think` o alteram.
    pub effort: SharedEffort,
    pub auto_yes: bool,
    pub color: bool,
}

// Como criar o cliente de um modelo, para a troca em tempo de execução. Roda na thread do agente.
pub type ModelFactory = Box<dyn Fn(&ModelInfo) -> Result<Box<dyn ChatModel>, String> + Send>;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------- estado da interface ----------

// Um turno em andamento.
struct Work {
    started: Instant,
    activity: String,
    // Pedido de aprovação aguardando resposta.
    approval: Option<Sender<bool>>,
    cancelling: bool,
}

struct App {
    terminal: DefaultTerminal,
    editor: TextArea<'static>,
    work: Option<Work>,
    usage: Option<Usage>,
    settings: Settings,
    commands: Sender<Command>,
    events: Receiver<UiEvent>,
    cancel: Arc<AtomicBool>,
    // Altura atual da área fixa e item escolhido no menu de comandos.
    height: u16,
    menu_index: usize,
    history: History,
    // Painel de skills aberto, se houver.
    picker: Option<Picker>,
    // Painel de modelos aberto, se houver.
    model_picker: Option<ModelPicker>,
}

// Roda a interface até o usuário sair. `make_session` roda na thread do agente.
pub fn run<F>(settings: Settings, make_session: F, make_model: ModelFactory) -> Result<(), String>
where
    F: FnOnce(Box<dyn Approver>) -> Result<Session, String> + Send + 'static,
{
    theme::set_color(settings.color);
    let (commands, command_rx) = mpsc::channel();
    let (event_tx, events) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let auto_yes = settings.auto_yes;
        let active = Arc::clone(&settings.model);
        let effort = Arc::clone(&settings.effort);
        thread::spawn(move || {
            worker(make_session, make_model, auto_yes, command_rx, event_tx, cancel, active, effort)
        });
    }

    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    let height = viewport_height(1, 0);
    let terminal = ratatui::init_with_options(TerminalOptions { viewport: Viewport::Inline(height) });
    let _ = execute!(io::stdout(), EnableBracketedPaste);
    if enhanced {
        let _ = execute!(io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }

    let mut app = App {
        terminal,
        editor: new_editor(),
        work: None,
        usage: None,
        settings,
        commands,
        events,
        cancel,
        height,
        menu_index: 0,
        history: History::default(),
        picker: None,
        model_picker: None,
    };
    let result = app.event_loop();

    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    // Sem `clear()`: ele apagaria a tela a partir do topo e a conversa visível sumiria.
    // A área do TUI fica no lugar; o cursor só desce para uma linha abaixo dela.
    ratatui::restore();
    println!();
    result
}

fn new_editor() -> TextArea<'static> {
    let mut ta = TextArea::default();
    ta.set_cursor_line_style(Style::default());
    ta.set_placeholder_text("Digite uma tarefa…");
    ta.set_placeholder_style(fg(GRAY));
    ta
}

impl App {
    fn event_loop(&mut self) -> Result<(), String> {
        self.print_header().map_err(|e| e.to_string())?;
        loop {
            self.fit_viewport().map_err(|e| e.to_string())?;
            let menu = self.menu_matches();
            let App { terminal, editor, work, usage, settings, menu_index, picker, model_picker, .. } = self;
            let view = View {
                editor,
                work: work.as_ref(),
                usage: *usage,
                settings,
                menu,
                menu_index: *menu_index,
                picker: picker.as_ref(),
                model_picker: model_picker.as_ref(),
            };
            terminal.draw(|f| draw(f, &view)).map_err(|e| e.to_string())?;
            if event::poll(Duration::from_millis(80)).map_err(|e| e.to_string())? {
                match event::read().map_err(|e| e.to_string())? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if self.on_key(key).map_err(|e| e.to_string())? {
                            return Ok(());
                        }
                    }
                    Event::Paste(text) if self.work.as_ref().is_none_or(|w| w.approval.is_none()) => {
                        self.editor.insert_str(text.replace("\r\n", "\n").replace('\r', "\n"));
                    }
                    _ => {}
                }
            }
            self.drain_events()?;
        }
    }

    // Comandos que combinam com o que está sendo digitado. Só aparecem com o editor ocioso e
    // uma única palavra começando por `/`.
    fn menu_matches(&self) -> Vec<&'static Cmd> {
        let lines = self.editor.lines();
        match lines {
            [line] if self.work.is_none() && line.starts_with('/') && !line.contains(char::is_whitespace) => {
                COMMANDS.iter().filter(|c| c.starts_with(line)).collect()
            }
            _ => Vec::new(),
        }
    }

    // Ajusta a altura da área fixa ao editor e ao menu. O viewport inline tem altura fixa,
    // então recriar o terminal é o jeito de mudá-la.
    fn fit_viewport(&mut self) -> io::Result<()> {
        let want = match (&self.model_picker, &self.picker) {
            (Some(mp), _) => picker_height(model_rows(mp).len()),
            (None, Some(p)) => picker_height(picker_rows(&self.settings.skills, p).len()),
            (None, None) => viewport_height(self.editor.lines().len(), self.menu_matches().len()),
        };
        if want != self.height {
            // Apaga a área antiga e deixa o cursor no topo dela: a nova começa no mesmo lugar,
            // sem deixar linhas em branco entre a transcrição e a área fixa.
            let top = self.terminal.get_frame().area().y;
            execute!(io::stdout(), MoveTo(0, top), Clear(ClearType::FromCursorDown))?;
            self.terminal = Terminal::with_options(
                CrosstermBackend::new(io::stdout()),
                TerminalOptions { viewport: Viewport::Inline(want) },
            )?;
            self.height = want;
        }
        Ok(())
    }

    // Teclas do painel de skills. Devolve `true` para sair (o painel nunca sai do programa).
    fn picker_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(p) = self.picker.as_ref() else { return Ok(false) };
        let rows = picker_rows(&self.settings.skills, p);
        let selected = rows.get(p.index).map(|&i| self.settings.skills[i].name.clone());
        let last = rows.len().saturating_sub(1);
        let Some(p) = self.picker.as_mut() else { return Ok(false) };
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.picker = None,
            KeyCode::Char('c') if ctrl => self.picker = None,
            KeyCode::Up => p.index = p.index.saturating_sub(1),
            KeyCode::Down => p.index = (p.index + 1).min(last),
            KeyCode::Tab => {
                p.by_name = !p.by_name;
                p.index = 0;
            }
            KeyCode::Backspace => {
                p.query.pop();
                p.index = 0;
            }
            KeyCode::Char(' ') => {
                if let Some(name) = selected {
                    let mut off = self.settings.skills_off.lock().unwrap_or_else(|e| e.into_inner());
                    if !off.remove(&name) {
                        off.insert(name);
                    }
                }
            }
            KeyCode::Char(c) if !ctrl => {
                p.query.push(c);
                p.index = 0;
            }
            _ => {}
        }
        Ok(false)
    }

    // Trata uma tecla. Devolve `true` para sair.
    fn on_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Decisão pendente: só as teclas de resposta valem.
        if let Some(work) = &mut self.work
            && let Some(reply) = work.approval.take()
        {
            match key.code {
                KeyCode::Char('s' | 'S' | 'y' | 'Y') if !ctrl => {
                    let _ = reply.send(true);
                }
                KeyCode::Char('c') if ctrl => {
                    work.cancelling = true;
                    self.cancel.store(true, Ordering::Relaxed);
                    let _ = reply.send(false);
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc | KeyCode::Enter => {
                    let _ = reply.send(false);
                }
                _ => work.approval = Some(reply),
            }
            return Ok(false);
        }

        // Painel de skills aberto: ele recebe todas as teclas, inclusive ↑/↓ (que, sem isso, iam para o histórico do editor).
        if self.model_picker.is_some() {
            return self.model_picker_key(key);
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }

        let menu = self.menu_matches();
        if menu.is_empty() && self.work.is_none() {
            let row = self.editor.cursor().0;
            let last_row = self.editor.lines().len().saturating_sub(1);
            let recalled = match key.code {
                KeyCode::Up if row == 0 => {
                    let current = self.editor.lines().join("\n");
                    self.history.up(&current)
                }
                KeyCode::Down if row == last_row => self.history.down(),
                _ => None,
            };
            if let Some(text) = recalled {
                self.editor = new_editor();
                self.editor.insert_str(text);
                return Ok(false);
            }
        }
        if !menu.is_empty() {
            let n = menu.len();
            let chosen = self.menu_index.min(n - 1);
            match key.code {
                KeyCode::Up => {
                    self.menu_index = (chosen + n - 1) % n;
                    return Ok(false);
                }
                KeyCode::Down => {
                    self.menu_index = (chosen + 1) % n;
                    return Ok(false);
                }
                KeyCode::Tab => {
                    self.editor = new_editor();
                    self.editor.insert_str(menu[chosen].name);
                    self.menu_index = 0;
                    return Ok(false);
                }
                KeyCode::Enter if !key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                    return self.submit(menu[chosen].name);
                }
                KeyCode::Esc => {
                    self.editor = new_editor();
                    return Ok(false);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('c') if ctrl => {
                if self.work.is_some() {
                    self.interrupt();
                } else if self.editor.is_empty() {
                    return Ok(true);
                } else {
                    self.editor = new_editor();
                }
            }
            KeyCode::Char('d') if ctrl && self.editor.is_empty() => return Ok(true),
            KeyCode::Esc => self.interrupt(),
            KeyCode::Char('j') if ctrl => self.editor.insert_newline(),
            KeyCode::Enter if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                self.editor.insert_newline()
            }
            KeyCode::Enter => {
                if self.work.is_none() {
                    let text = self.editor.lines().join("\n").trim().to_string();
                    if !text.is_empty() {
                        return self.submit(&text);
                    }
                }
            }
            _ => {
                self.editor.input(key);
                self.menu_index = 0;
                self.history.edited();
            }
        }
        Ok(false)
    }

    fn interrupt(&mut self) {
        if let Some(work) = &mut self.work {
            work.cancelling = true;
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    // Envia o texto do editor: comando `/` ou tarefa para o agente. Devolve `true` para sair.
    fn submit(&mut self, text: &str) -> io::Result<bool> {
        self.editor = new_editor();
        self.history.push(text);
        // Comando com argumento: `/model <id>` e `/effort <nível>`. O nome decide o comando; o resto é o argumento.
        let (head, arg) = text.split_once(char::is_whitespace).map_or((text, ""), |(h, a)| (h, a.trim()));
        let command = COMMANDS
            .iter()
            .find(|c| c.is(head))
            .filter(|c| arg.is_empty() || ARG_COMMANDS.contains(&c.name));
        match text {
            _ if command.is_some_and(|c| c.name == "/exit") => return Ok(true),
            _ if command.is_some_and(|c| c.name == "/clear") => {
                let _ = self.commands.send(Command::Clear);
                self.clear_screen()?;
            }
            _ if command.is_some_and(|c| c.name == "/think") => self.think_command()?,
            _ if command.is_some_and(|c| c.name == "/model") => self.model_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/effort") => self.effort_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/init") => self.init_command()?,
            _ if command.is_some_and(|c| c.name == "/skills") => {
                if self.settings.skills.is_empty() {
                    self.print(vec![note("nenhuma skill carregada."), Line::default()])?;
                } else {
                    self.picker = Some(Picker { query: String::new(), index: 0, by_name: false });
                }
            }
            _ if command.is_some_and(|c| c.name == "/help") => {
                let mut lines: Vec<Line<'static>> =
                    COMMANDS.iter().map(|c| note(&format!("{:<8} {}", c.name, c.desc))).collect();
                lines.push(note("enter envia · shift+enter ou ctrl+j nova linha · esc interrompe"));
                lines.push(Line::default());
                self.print(lines)?;
            }
            command if command.starts_with('/') && !command.contains(char::is_whitespace) => {
                self.print(vec![
                    Line::from(Span::styled(format!("  comando desconhecido: {command}. Veja /help."), fg(YELLOW))),
                    Line::default(),
                ])?;
            }
            task => {
                let width = self.width()?;
                let mut lines = markdown::plain(task, width, Span::styled("› ", bold(YELLOW)), Style::default(), &palette());
                lines.push(Line::default());
                self.print(lines)?;
                self.cancel.store(false, Ordering::Relaxed);
                let _ = self.commands.send(Command::Submit(task.to_string()));
                self.work =
                    Some(Work { started: Instant::now(), activity: "pensando".into(), approval: None, cancelling: false });
            }
        }
        Ok(false)
    }

    // `/clear`: apaga a tela e o scrollback e recomeça com o cabeçalho, como no Claude Code.
    fn active_model(&self) -> ModelInfo {
        locked(&self.settings.model).clone()
    }

    fn switch_model(&self, model: ModelInfo) {
        let _ = self.commands.send(Command::SwitchModel(model));
    }

    // `/think`: liga no padrão do modelo ou desliga. É um atalho de `/effort`.
    fn think_command(&mut self) -> io::Result<()> {
        let model = self.active_model();
        if matches!(model.capabilities.reasoning, Reasoning::Unsupported) {
            return self.print(vec![note("este modelo não tem raciocínio configurável."), Line::default()]);
        }
        let next = model.capabilities.toggle_think(*locked(&self.settings.effort));
        *locked(&self.settings.effort) = next;
        if next.is_off() {
            return self.print(vec![note("thinking desligado."), Line::default()]);
        }
        let mut lines = self.warning(brand::THINK_WARNING)?;
        lines.push(Line::default());
        self.print(lines)
    }

    // `/init`: o agente monta o rascunho e pede aprovação; a prévia aparece como as outras edições.
    fn init_command(&mut self) -> io::Result<()> {
        if self.work.is_some() {
            return self.print(vec![note("espere a tarefa terminar para criar o AGENTS.md."), Line::default()]);
        }
        self.cancel.store(false, Ordering::Relaxed);
        let _ = self.commands.send(Command::Init);
        self.work = Some(Work { started: Instant::now(), activity: "montando AGENTS.md".into(), approval: None, cancelling: false });
        Ok(())
    }

    // `/effort`: sem argumento mostra o nível atual e os níveis deste modelo; com um, valida e aplica.
    fn effort_command(&mut self, arg: &str) -> io::Result<()> {
        let model = self.active_model();
        if arg.is_empty() {
            let current = *locked(&self.settings.effort);
            let levels: Vec<&str> = model.capabilities.supported_efforts().into_iter().map(Effort::as_str).collect();
            let text = format!("esforço {} · níveis deste modelo: {}", current.as_str(), levels.join(", "));
            return self.print(vec![note(&text), Line::default()]);
        }
        let text = match model.capabilities.parse_for(arg) {
            Ok(effort) => {
                *locked(&self.settings.effort) = effort;
                format!("esforço: {}", effort.as_str())
            }
            Err(msg) => msg,
        };
        self.print(vec![note(&text), Line::default()])
    }

    // `/model`: sem argumento abre o painel; com um id, troca direto. A troca espera a tarefa atual.
    fn model_command(&mut self, arg: &str) -> io::Result<()> {
        if self.work.is_some() {
            return self.print(vec![note("espere a tarefa terminar para trocar de modelo."), Line::default()]);
        }
        if !arg.is_empty() {
            return match self.settings.catalog.find(arg) {
                Ok(Some(model)) => {
                    self.switch_model(model);
                    Ok(())
                }
                Ok(None) => self.print(vec![note(&format!("modelo não instalado: {arg}. Veja /model.")), Line::default()]),
                Err(e) => self.print(vec![note(&format!("não consegui consultar os modelos: {e}")), Line::default()]),
            };
        }
        match self.settings.catalog.list() {
            Ok(models) if models.is_empty() => {
                self.print(vec![note("nenhum modelo instalado. Instale com: ollama pull <modelo>"), Line::default()])
            }
            Ok(models) => {
                self.model_picker = Some(ModelPicker { query: String::new(), index: 0, models });
                Ok(())
            }
            Err(e) => self.print(vec![note(&format!("não consegui listar os modelos: {e}")), Line::default()]),
        }
    }

    // Teclas do painel de modelos. Enter escolhe e fecha; Esc fecha sem trocar. Devolve `true` para sair (nunca sai).
    fn model_picker_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(mp) = self.model_picker.as_mut() else { return Ok(false) };
        let rows = model_rows(mp);
        let last = rows.len().saturating_sub(1);
        let chosen = rows.get(mp.index).map(|&i| mp.models[i].clone());
        match key.code {
            KeyCode::Esc => self.model_picker = None,
            KeyCode::Char('c') if ctrl => self.model_picker = None,
            KeyCode::Enter => {
                self.model_picker = None;
                if let Some(model) = chosen {
                    self.switch_model(model);
                }
            }
            KeyCode::Up => mp.index = mp.index.saturating_sub(1),
            KeyCode::Down => mp.index = (mp.index + 1).min(last),
            KeyCode::Backspace => {
                mp.query.pop();
                mp.index = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                mp.query.push(c);
                mp.index = 0;
            }
            _ => {}
        }
        Ok(false)
    }

    fn clear_screen(&mut self) -> io::Result<()> {
        execute!(io::stdout(), Clear(ClearType::All), Clear(ClearType::Purge), MoveTo(0, 0))?;
        self.terminal = Terminal::with_options(
            CrosstermBackend::new(io::stdout()),
            TerminalOptions { viewport: Viewport::Inline(self.height) },
        )?;
        self.print_header()?;
        self.print(vec![note("contexto limpo; o prompt do sistema foi mantido."), Line::default()])
    }

    // Aplica os eventos que o agente mandou.
    fn drain_events(&mut self) -> Result<(), String> {
        loop {
            let event = match self.events.try_recv() {
                Ok(e) => e,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err("o agente parou inesperadamente".into()),
            };
            match event {
                UiEvent::Activity(a) => {
                    if let Some(w) = &mut self.work
                        && !w.cancelling
                    {
                        w.activity = a;
                    }
                }
                UiEvent::Usage(u) => self.usage = Some(u),
                UiEvent::Approval { preview, reply } => {
                    self.print_preview(&preview).map_err(|e| e.to_string())?;
                    match &mut self.work {
                        Some(w) => w.approval = Some(reply),
                        None => {
                            let _ = reply.send(false);
                        }
                    }
                }
                UiEvent::Done { result, usage } => {
                    self.usage = Some(usage);
                    self.work = None;
                    match result {
                        Ok(report) => self.print_report(&report),
                        Err(e) => self.print(vec![
                            Line::from(Span::styled(format!("  erro: {e}"), fg(RED))),
                            Line::default(),
                        ]),
                    }
                    .map_err(|e| e.to_string())?;
                }
                UiEvent::Notice(text) => {
                    self.print(vec![note(&text), Line::default()]).map_err(|e| e.to_string())?;
                }
                UiEvent::InitDone(result) => {
                    self.work = None;
                    let text = match result {
                        Ok(message) => message,
                        Err(e) => format!("erro: {e}"),
                    };
                    self.print(vec![note(&text), Line::default()]).map_err(|e| e.to_string())?;
                }
                UiEvent::Fatal(e) => return Err(e),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitclaw::models::ModelCapabilities;
    use bitclaw::skills;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn settings(auto_yes: bool) -> Settings {
        Settings {
            title: "BitClaw".into(),
            info: vec![],
            skills: vec![],
            skills_off: skills::new_disabled(),
            model: Arc::new(Mutex::new(ModelInfo::unknown("m:1b"))),
            default_model: "m:1b".into(),
            catalog: Arc::new(StaticCatalog(Vec::new())),
            effort: Arc::new(Mutex::new(Effort::Off)),
            auto_yes,
            color: false,
        }
    }

    // Desenha a área fixa com o editor contendo `typed` e devolve as linhas de texto.
    fn screen_with(width: u16, typed: &str, work: Option<&Work>, usage: Option<Usage>, auto_yes: bool) -> Vec<String> {
        let mut editor = new_editor();
        editor.insert_str(typed);
        let menu: Vec<&'static Cmd> = match editor.lines() {
            [line] if work.is_none() && line.starts_with('/') => COMMANDS.iter().filter(|c| c.starts_with(line)).collect(),
            _ => Vec::new(),
        };
        let height = viewport_height(editor.lines().len(), menu.len());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let settings = settings(auto_yes);
        let view = View { editor: &editor, work, usage, settings: &settings, menu, menu_index: 0, picker: None, model_picker: None };
        terminal.draw(|f| draw(f, &view)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect()
    }

    fn screen(work: Option<&Work>, usage: Option<Usage>, auto_yes: bool) -> Vec<String> {
        screen_with(80, "", work, usage, auto_yes)
    }

    fn work(approval: bool, cancelling: bool) -> Work {
        Work {
            started: Instant::now(),
            activity: "executando read_file a.txt".into(),
            approval: approval.then(|| mpsc::channel().0),
            cancelling,
        }
    }

    #[test]
    fn idle_screen_is_compact_with_one_editor_row() {
        let usage = Usage { tokens: 861, window: 16384, percent: 5.0 };
        let lines = screen(None, Some(usage), false);
        assert_eq!(lines.len(), 5, "status, régua, 1 linha de editor, régua e rodapé: {lines:?}");
        assert_eq!(lines[0], "", "sem status quando ocioso");
        assert!(lines[1].starts_with('─') && lines[3].starts_with('─'));
        assert!(lines[2].starts_with('›') && lines[2].contains("Digite uma tarefa…"), "{lines:?}");
        assert!(lines[4].starts_with("m:1b · Manual · contexto 5% (861/16384)"), "{}", lines[4]);
        assert!(lines[4].ends_with("enter envia · / comandos"), "versão curta dos atalhos em 80 colunas");
    }

    #[test]
    fn editor_grows_with_the_text_up_to_a_limit() {
        assert_eq!(screen_with(80, "a\nb\nc", None, None, false).len(), 7);
        assert_eq!(screen_with(80, &"x\n".repeat(20), None, None, false).len(), 5 + MAX_EDITOR_ROWS - 1);
    }

    #[test]
    fn skills_picker_filters_by_name_and_description_and_sorts_by_name() {
        let skill = |name: &str, desc: &str| Skill {
            name: name.into(),
            description: desc.into(),
            dir: Default::default(),
            body: String::new(),
            files: vec![],
            builtin: false,
        };
        let skills = vec![skill("zeta", "gera relatórios"), skill("alfa", "revisa testes")];
        let mut p = Picker { query: String::new(), index: 0, by_name: false };
        assert_eq!(picker_rows(&skills, &p), vec![0, 1], "sem busca, ordem original");
        p.by_name = true;
        assert_eq!(picker_rows(&skills, &p), vec![1, 0], "ordenado por nome");
        p.by_name = false;
        p.query = "TESTES".into();
        assert_eq!(picker_rows(&skills, &p), vec![1], "busca ignora maiúsculas e olha a descrição");
        p.query = "nada".into();
        assert!(picker_rows(&skills, &p).is_empty());
    }

    #[test]
    fn typing_a_slash_lists_every_command_below_the_editor() {
        let lines = screen_with(100, "/", None, None, false);
        assert_eq!(lines.len(), 5 + COMMANDS.len());
        assert!(lines[4].starts_with("› /help") && lines[4].contains("mostra os comandos"), "{lines:?}");
        assert!(lines[5].starts_with("  /model") && lines[6].starts_with("  /effort"), "{lines:?}");
        assert!(lines[7].starts_with("  /init"), "{lines:?}");
        assert!(lines[8].starts_with("  /clear") && lines[9].starts_with("  /think"), "{lines:?}");
        assert!(lines[10].starts_with("  /skills"), "{lines:?}");
        assert!(lines[11].starts_with("  /exit"), "{lines:?}");
        assert!(lines.last().unwrap().contains("tab completa · enter executa"));
    }

    #[test]
    fn the_menu_filters_by_name() {
        let lines = screen_with(100, "/cl", None, None, false);
        assert_eq!(lines.len(), 6);
        assert!(lines[4].starts_with("› /clear"), "{lines:?}");
        let lines = screen_with(100, "/ex", None, None, false);
        assert!(lines[4].starts_with("› /exit"), "{lines:?}");
        // Sem correspondência, não há menu.
        assert_eq!(screen_with(100, "/zzz", None, None, false).len(), 5);
    }

    #[test]
    fn the_think_command_explains_its_cost_in_the_menu() {
        let lines = screen_with(120, "/th", None, None, false);
        assert!(lines[4].starts_with("› /think") && lines[4].contains("gasta mais contexto"), "{lines:?}");
    }

    #[test]
    fn footer_flags_thinking_only_when_it_is_on() {
        let off = screen_with(100, "", None, None, false).pop().unwrap();
        assert!(!off.contains("esforço"), "{off}");
        let mut editor = new_editor();
        editor.insert_str("");
        let cfg = settings(false);
        *cfg.effort.lock().unwrap() = Effort::Medium;
        let mut terminal = Terminal::new(TestBackend::new(100, viewport_height(1, 0))).unwrap();
        let view = View { editor: &editor, work: None, usage: None, settings: &cfg, menu: Vec::new(), menu_index: 0, picker: None, model_picker: None };
        terminal.draw(|f| draw(f, &view)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..100).map(|x| buffer[(x, 4)].symbol().to_string()).collect();
        assert!(footer.starts_with("m:1b · Manual · esforço medium · contexto"), "{footer}");
    }

    #[test]
    fn only_english_command_names_resolve() {
        let name = |t: &str| COMMANDS.iter().find(|c| c.is(t)).map(|c| c.name);
        assert_eq!(name("/clear"), Some("/clear"));
        assert_eq!(name("/quit"), Some("/exit"));
        assert_eq!(name("/limpar-contexto"), None);
        assert_eq!(name("/limpar"), None);
        assert_eq!(name("/sair"), None);
        assert_eq!(name("/ajuda"), None);
    }

    // Catálogo fixo para os testes: sem rede.
    struct StaticCatalog(Vec<ModelInfo>);

    impl ModelCatalog for StaticCatalog {
        fn list(&self) -> Result<Vec<ModelInfo>, String> {
            Ok(self.0.clone())
        }
    }

    fn sample_models() -> Vec<ModelInfo> {
        let levels = ModelCapabilities {
            tool_calling: true,
            reasoning: Reasoning::Levels(vec![Effort::Low, Effort::Medium, Effort::High]),
            context_window: Some(32768),
        };
        let plain = ModelCapabilities { tool_calling: false, reasoning: Reasoning::Unsupported, context_window: None };
        vec![
            ModelInfo { id: "gpt-oss:20b".into(), provider: "ollama".into(), capabilities: levels },
            ModelInfo { id: "qwen2.5-coder:3b".into(), provider: "ollama".into(), capabilities: plain },
        ]
    }

    #[test]
    fn model_picker_search_matches_the_id_only() {
        let typed = ModelPicker { query: "CODER".into(), index: 0, models: sample_models() };
        assert_eq!(model_rows(&typed), vec![1]);
        let all = ModelPicker { query: String::new(), index: 0, models: sample_models() };
        assert_eq!(model_rows(&all), vec![0, 1]);
        let none = ModelPicker { query: "zzz".into(), index: 0, models: sample_models() };
        assert!(model_rows(&none).is_empty());
    }

    #[test]
    fn model_picker_marks_the_active_and_the_default_model() {
        let models = sample_models();
        let mut cfg = settings(false);
        cfg.default_model = "gpt-oss:20b".into();
        *cfg.model.lock().unwrap() = models[1].clone();
        let picker = ModelPicker { query: String::new(), index: 0, models };
        let mut terminal = Terminal::new(TestBackend::new(120, picker_height(2))).unwrap();
        terminal.draw(|f| draw_model_picker(f, &cfg, &picker)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let row = |y: u16| -> String { (0..120).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string() };
        assert!(row(0).contains("2 modelos · ativo qwen2.5-coder:3b · padrão gpt-oss:20b"), "{}", row(0));
        assert!(row(4).contains("gpt-oss:20b") && row(4).contains("raciocínio: low, medium, high"), "{}", row(4));
        assert!(row(4).ends_with("· padrão"), "{}", row(4));
        assert!(row(5).contains("● qwen2.5-coder:3b") && row(5).contains("sem ferramentas"), "{}", row(5));
    }

    #[test]
    fn footer_shows_the_automatic_mode_and_unknown_usage() {
        let lines = screen(None, None, true);
        assert!(lines[4].starts_with("m:1b · Automático · contexto —"), "{}", lines[4]);
    }

    #[test]
    fn working_screen_shows_the_activity_and_how_to_interrupt() {
        let lines = screen(Some(&work(false, false)), None, false);
        assert!(lines[0].contains("executando read_file a.txt… 0s"), "{}", lines[0]);
        assert!(lines[4].ends_with("esc interrompe"));
        let lines = screen(Some(&work(false, true)), None, false);
        assert!(lines[0].contains("interrompendo, aguardando o modelo"), "{}", lines[0]);
    }

    #[test]
    fn approval_screen_asks_for_a_decision() {
        let lines = screen(Some(&work(true, false)), None, false);
        assert!(lines[0].contains("Aplicar esta alteração? [s] sim · [n] não"), "{}", lines[0]);
        assert!(lines[4].ends_with("s aplica · n recusa"));
    }

    #[test]
    fn footer_drops_the_hints_when_the_window_is_narrow() {
        let footer = screen_with(40, "", None, None, false).pop().unwrap();
        assert!(footer.starts_with("m:1b · Manual · contexto —"));
        assert!(!footer.contains("enter envia"), "{footer}");
    }
}

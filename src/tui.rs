//! Interface de terminal do BitClaw, com ratatui em viewport inline.
//!
//! O cabeçalho e a transcrição entram no scrollback normal do terminal (rolar, selecionar e
//! copiar funcionam como sempre). Só uma área fixa fica embaixo: status, editor e rodapé.
//!
//! O agente roda numa thread própria e fala com a interface por canais. Só este módulo escreve
//! na tela: a biblioteca continua sem imprimir nada.

use crate::brand;
use crate::markdown::{self, Palette};
use bitclaw::agent::{INTERRUPTED, Progress, TurnReport};
use bitclaw::context::Usage;
use bitclaw::session::Session;
use bitclaw::tools::{Approver, AutoApprover, Outcome, Preview, PreviewLine};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::cursor::MoveTo;
use crossterm::execute;
use crossterm::terminal::{Clear, ClearType};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::backend::CrosstermBackend;
use ratatui::{DefaultTerminal, Frame, Terminal, TerminalOptions, Viewport};
use ratatui_textarea::TextArea;
use serde_json::Value;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const YELLOW: Color = Color::Rgb(255, 204, 0);
const GRAY: Color = Color::Rgb(150, 150, 150);
const GREEN: Color = Color::Rgb(0, 200, 120);
const RED: Color = Color::Rgb(255, 77, 77);
const ORANGE: Color = Color::Rgb(255, 150, 60);
const CODE: Color = Color::Rgb(200, 200, 200);

// A área fixa cresce e encolhe: status (1) + régua, editor e régua (2 + linhas do editor)
// + menu de comandos + rodapé (1).
const MAX_EDITOR_ROWS: usize = 6;

fn viewport_height(editor_rows: usize, menu_rows: usize) -> u16 {
    (1 + 2 + editor_rows.clamp(1, MAX_EDITOR_ROWS) + menu_rows + 1) as u16
}

// Comandos com `/`. O nome é em inglês, como no Claude Code; os nomes antigos em português
// continuam funcionando como apelidos.
struct Cmd {
    name: &'static str,
    aliases: &'static [&'static str],
    desc: &'static str,
}

const COMMANDS: [Cmd; 4] = [
    Cmd { name: "/help", aliases: &[], desc: "mostra os comandos e as teclas" },
    Cmd { name: "/clear", aliases: &[], desc: "apaga o histórico e mantém o prompt do sistema" },
    Cmd { name: "/think", aliases: &[], desc: "liga ou desliga o raciocínio do modelo (gasta mais contexto)" },
    Cmd { name: "/exit", aliases: &["/quit"], desc: "encerra o BitClaw" },
];

impl Cmd {
    fn is(&self, text: &str) -> bool {
        self.name == text || self.aliases.contains(&text)
    }

    fn starts_with(&self, prefix: &str) -> bool {
        self.name.starts_with(prefix) || self.aliases.iter().any(|a| a.starts_with(prefix))
    }
}

static COLOR: AtomicBool = AtomicBool::new(true);

// Estilo com a cor pedida, ou sem cor se `--no-color` ou `NO_COLOR` estiverem ativos.
fn fg(color: Color) -> Style {
    if COLOR.load(Ordering::Relaxed) { Style::default().fg(color) } else { Style::default() }
}

fn bold(color: Color) -> Style {
    fg(color).add_modifier(Modifier::BOLD)
}

fn palette() -> Palette {
    Palette {
        text: Style::default(),
        code: fg(CODE),
        heading: bold(YELLOW),
        bullet: fg(YELLOW),
    }
}

pub struct Settings {
    pub title: String,
    // Linhas ao lado do mascote, abaixo do nome.
    pub info: Vec<String>,
    pub model: String,
    pub auto_yes: bool,
    pub color: bool,
    // Raciocínio do modelo ligado ou não. Compartilhado com o cliente; o `/think` o alterna.
    pub think: Arc<AtomicBool>,
}

// ---------- comunicação com o agente ----------

enum Command {
    Submit(String),
    Clear,
}

enum UiEvent {
    Activity(String),
    Approval { preview: Preview, reply: Sender<bool> },
    Done { result: Result<TurnReport, String>, usage: Usage },
    Usage(Usage),
    Fatal(String),
}

// Sinais do agente viram eventos para a interface.
struct ChannelProgress {
    tx: Sender<UiEvent>,
    cancel: Arc<AtomicBool>,
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
struct ChannelApprover {
    tx: Sender<UiEvent>,
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
fn worker<F>(make: F, auto_yes: bool, commands: Receiver<Command>, tx: Sender<UiEvent>, cancel: Arc<AtomicBool>)
where
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
        }
    }
}

// ---------- estado da interface ----------

// Histórico das tarefas enviadas, navegado com ↑ e ↓ como no shell. Sem terminal: só texto.
#[derive(Default)]
struct History {
    items: Vec<String>,
    // Posição atual ao navegar; `None` quando o editor mostra o próprio texto.
    pos: Option<usize>,
    // O que estava digitado antes de começar a navegar, para voltar a ele ao descer até o fim.
    draft: String,
}

impl History {
    fn push(&mut self, text: &str) {
        if !text.is_empty() && self.items.last().map(String::as_str) != Some(text) {
            self.items.push(text.to_string());
        }
        self.pos = None;
    }

    // ↑: volta uma entrada. `current` é o texto do editor, guardado se for a primeira volta.
    fn up(&mut self, current: &str) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }
        let next = match self.pos {
            None => {
                self.draft = current.to_string();
                self.items.len() - 1
            }
            Some(p) => p.saturating_sub(1),
        };
        self.pos = Some(next);
        Some(self.items[next].clone())
    }

    // ↓: avança uma entrada; depois da última, volta ao texto que estava sendo digitado.
    fn down(&mut self) -> Option<String> {
        let p = self.pos?;
        if p + 1 < self.items.len() {
            self.pos = Some(p + 1);
            Some(self.items[p + 1].clone())
        } else {
            self.pos = None;
            Some(std::mem::take(&mut self.draft))
        }
    }

    // Editar um item recuperado o torna o texto atual, sem voltar a navegar a partir dele.
    fn edited(&mut self) {
        self.pos = None;
    }
}

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
}

// Roda a interface até o usuário sair. `make_session` roda na thread do agente.
pub fn run<F>(settings: Settings, make_session: F) -> Result<(), String>
where
    F: FnOnce(Box<dyn Approver>) -> Result<Session, String> + Send + 'static,
{
    COLOR.store(settings.color, Ordering::Relaxed);
    let (commands, command_rx) = mpsc::channel();
    let (event_tx, events) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let cancel = Arc::clone(&cancel);
        let auto_yes = settings.auto_yes;
        thread::spawn(move || worker(make_session, auto_yes, command_rx, event_tx, cancel));
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
    };
    let result = app.event_loop();

    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    let _ = app.terminal.clear();
    ratatui::restore();
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
            let App { terminal, editor, work, usage, settings, menu_index, .. } = self;
            let view = View { editor, work: work.as_ref(), usage: *usage, settings, menu, menu_index: *menu_index };
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
        let want = viewport_height(self.editor.lines().len(), self.menu_matches().len());
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
        let command = COMMANDS.iter().find(|c| c.is(text));
        match text {
            _ if command.is_some_and(|c| c.name == "/exit") => return Ok(true),
            _ if command.is_some_and(|c| c.name == "/clear") => {
                let _ = self.commands.send(Command::Clear);
                self.clear_screen()?;
            }
            _ if command.is_some_and(|c| c.name == "/think") => {
                let on = !self.settings.think.fetch_xor(true, Ordering::Relaxed);
                let mut lines = if on { self.warning(brand::THINK_WARNING)? } else { vec![note("thinking desligado.")] };
                lines.push(Line::default());
                self.print(lines)?;
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
                UiEvent::Fatal(e) => return Err(e),
            }
        }
    }

    // ---------- saída para o scrollback ----------

    fn width(&mut self) -> io::Result<usize> {
        Ok(self.terminal.size()?.width as usize)
    }

    fn print(&mut self, lines: Vec<Line<'static>>) -> io::Result<()> {
        // Em pedaços, para nunca passar da altura da tela de uma vez.
        let chunk = usize::from(self.terminal.size()?.height.saturating_sub(self.height).max(1));
        for part in lines.chunks(chunk) {
            let part = part.to_vec();
            self.terminal
                .insert_before(part.len() as u16, |buf| Paragraph::new(part).render(buf.area, buf))?;
        }
        Ok(())
    }

    fn print_header(&mut self) -> io::Result<()> {
        let mut right = vec![Line::from(Span::styled(self.settings.title.clone(), bold(YELLOW)))];
        right.extend(self.settings.info.iter().map(|l| Line::from(Span::styled(l.clone(), fg(GRAY)))));
        let glyph = brand::crab();
        let offset = glyph.len().saturating_sub(right.len()) / 2;
        let mut lines: Vec<Line<'static>> = glyph
            .into_iter()
            .enumerate()
            .map(|(i, g)| {
                let mut spans = vec![Span::styled(g, fg(YELLOW)), Span::raw("  ")];
                if let Some(text) = i.checked_sub(offset).and_then(|j| right.get(j)) {
                    spans.extend(text.spans.clone());
                }
                Line::from(spans)
            })
            .collect();
        lines.push(Line::default());
        lines.push(note("Digite uma tarefa. Digite / para ver os comandos."));
        lines.push(Line::default());
        if self.settings.auto_yes {
            lines.insert(lines.len() - 1, aviso_line("--yes ativo: edições serão aplicadas sem confirmação"));
        }
        if self.settings.think.load(Ordering::Relaxed) {
            let at = lines.len() - 1;
            lines.splice(at..at, self.warning(brand::THINK_WARNING)?);
        }
        self.print(lines)
    }

    // Aviso em amarelo, quebrado na largura da janela.
    fn warning(&mut self, text: &str) -> io::Result<Vec<Line<'static>>> {
        let width = self.width()?;
        Ok(markdown::plain(text, width, Span::styled("  aviso: ", bold(ORANGE)), fg(ORANGE), &palette()))
    }

    // Prévia de uma alteração, com diff colorido, antes de pedir a decisão.
    fn print_preview(&mut self, preview: &Preview) -> io::Result<()> {
        let width = self.width()?.saturating_sub(4).max(10);
        let cut = |t: &str| -> String { t.chars().take(width).collect() };
        let mut lines = vec![Line::from(Span::styled(format!("── {} ──", preview.title), bold(YELLOW)))];
        for line in &preview.lines {
            lines.push(match line {
                PreviewLine::Context(t) => Line::from(Span::styled(format!("  {}", cut(t)), fg(GRAY))),
                PreviewLine::Removed(t) => Line::from(Span::styled(format!("- {}", cut(t)), fg(RED))),
                PreviewLine::Added(t) => Line::from(Span::styled(format!("+ {}", cut(t)), fg(GREEN))),
                PreviewLine::Note(t) => Line::from(Span::styled(cut(t), fg(RED))),
            });
        }
        self.print(lines)
    }

    // Relatório do turno: ações primeiro (o que foi executado de fato), depois o texto do modelo.
    fn print_report(&mut self, report: &TurnReport) -> io::Result<()> {
        let width = self.width()?;
        let mut lines: Vec<Line<'static>> = Vec::new();
        for t in &report.trims {
            lines.push(note(&format!(
                "[contexto] descartados {} turno(s) antigo(s); caracteres {} → {} (orçamento {})",
                t.turns, t.chars_before, t.chars_after, t.budget
            )));
        }
        for a in &report.actions {
            let target = a.args.get("path").and_then(Value::as_str).unwrap_or("");
            let (icon, color) = match a.outcome {
                Outcome::Done(_) => ("✓", GREEN),
                Outcome::Refused(_) => ("⊘", YELLOW),
                Outcome::Error(_) => ("✗", RED),
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(icon, fg(color)),
                Span::raw(format!(" {} ", a.name)),
                Span::styled(target.to_string(), fg(GRAY)),
            ]));
            if !matches!(a.outcome, Outcome::Done(_)) {
                let detail: String = a.outcome.text().lines().next().unwrap_or("").chars().take(width.saturating_sub(6)).collect();
                lines.push(Line::from(Span::styled(format!("      {detail}"), fg(GRAY))));
            }
        }
        let interrupted = report.error.as_deref() == Some(INTERRUPTED);
        if report.text.is_empty() {
            if !interrupted {
                lines.push(Line::default());
                lines.push(note("(sem resposta)"));
            }
        } else {
            lines.push(Line::default());
            lines.extend(markdown::render(&report.text, width, &palette()));
        }
        if let Some(e) = &report.error {
            lines.push(if interrupted {
                note("interrompido")
            } else {
                Line::from(Span::styled(format!("  interrompido: {e}"), fg(RED)))
            });
        }
        for w in report.warnings() {
            lines.push(aviso_line(&w));
        }
        lines.push(Line::default());
        self.print(lines)
    }
}

// Aviso: rótulo "aviso:" em laranja, sem glifo.
fn aviso_line(text: &str) -> Line<'static> {
    Line::from(vec![Span::styled("  aviso: ", bold(ORANGE)), Span::styled(text.to_string(), fg(ORANGE))])
}

// Linha cinza com o recuo padrão.
fn note(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!("  {text}"), fg(GRAY)))
}

// ---------- área fixa embaixo ----------

// Tudo o que `draw` precisa, emprestado do estado da interface.
struct View<'a> {
    editor: &'a TextArea<'static>,
    work: Option<&'a Work>,
    usage: Option<Usage>,
    settings: &'a Settings,
    menu: Vec<&'static Cmd>,
    menu_index: usize,
}

fn draw(f: &mut Frame, v: &View) {
    let editor_rows = v.editor.lines().len().clamp(1, MAX_EDITOR_ROWS) as u16;
    let [status, composer, menu, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(editor_rows + 2),
        Constraint::Length(v.menu.len() as u16),
        Constraint::Length(1),
    ])
    .areas(f.area());

    f.render_widget(Paragraph::new(status_line(v.work)), status);

    // Editor delimitado por réguas, como no Claude Code.
    let block = Block::new().borders(Borders::TOP | Borders::BOTTOM).border_style(fg(GRAY));
    let inner = block.inner(composer);
    f.render_widget(block, composer);
    let [mark, text] = Layout::horizontal([Constraint::Length(2), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new("›").style(bold(YELLOW)), mark);
    f.render_widget(v.editor, text);

    // Menu de comandos, abaixo do editor.
    let chosen = v.menu_index.min(v.menu.len().saturating_sub(1));
    let rows: Vec<Line> = v
        .menu
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (mark, name) = if i == chosen { ("› ", bold(YELLOW)) } else { ("  ", fg(GRAY)) };
            Line::from(vec![
                Span::styled(mark, name),
                Span::styled(format!("{:<8} ", c.name), name),
                Span::styled(c.desc, fg(GRAY)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(rows), menu);

    // Rodapé: modelo, modo e contexto à esquerda; atalhos à direita, na versão que couber.
    let mut info = vec![Span::styled(
        format!("{} · {} · ", v.settings.model, if v.settings.auto_yes { "Automático" } else { "Manual" }),
        fg(GRAY),
    )];
    if v.settings.think.load(Ordering::Relaxed) {
        info.push(Span::styled("thinking", fg(YELLOW)));
        info.push(Span::styled(" · ", fg(GRAY)));
    }
    info.push(Span::styled("contexto ", fg(GRAY)));
    match v.usage {
        Some(u) => {
            let color = if u.percent >= 90.0 { RED } else if u.percent >= 70.0 { YELLOW } else { GREEN };
            info.push(Span::styled(format!("{:.0}%", u.percent), fg(color)));
            info.push(Span::styled(format!(" ({}/{})", u.tokens, u.window), fg(GRAY)));
        }
        None => info.push(Span::styled("—", fg(GRAY))),
    }
    let info_width: usize = info.iter().map(Span::width).sum();
    let room = (footer.width as usize).saturating_sub(info_width + 2);
    let hints: &[&str] = match v.work {
        _ if !v.menu.is_empty() => &["↑↓ escolhe · tab completa · enter executa", "tab completa · enter executa", ""],
        None => &["enter envia · shift+enter nova linha · / comandos", "enter envia · / comandos", ""],
        Some(w) if w.approval.is_some() => &["s aplica · n recusa", ""],
        Some(_) => &["esc interrompe", ""],
    };
    let hint = hints.iter().find(|h| h.chars().count() <= room).copied().unwrap_or("");
    let [left, right] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(hint.chars().count() as u16)]).areas(footer);
    f.render_widget(Paragraph::new(Line::from(info)), left);
    f.render_widget(Paragraph::new(hint).style(fg(GRAY)), right);
}

// Spinner e atividade do turno, ou a pergunta de aprovação.
fn status_line(work: Option<&Work>) -> Line<'static> {
    let Some(work) = work else { return Line::default() };
    if work.approval.is_some() {
        return Line::from(Span::styled("▸ Aplicar esta alteração? [s] sim · [n] não", bold(YELLOW)));
    }
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let elapsed = work.started.elapsed();
    let frame = FRAMES[(elapsed.as_millis() / 80) as usize % FRAMES.len()];
    let label = if work.cancelling {
        "interrompendo, aguardando o modelo".to_string()
    } else {
        work.activity.clone()
    };
    Line::from(vec![
        Span::styled(format!("{frame} "), fg(YELLOW)),
        Span::styled(format!("{label}… {}s", elapsed.as_secs()), fg(GRAY)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn settings(auto_yes: bool) -> Settings {
        Settings {
            title: "BitClaw".into(),
            info: vec![],
            model: "m:1b".into(),
            auto_yes,
            color: false,
            think: Arc::new(AtomicBool::new(false)),
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
        let view = View { editor: &editor, work, usage, settings: &settings, menu, menu_index: 0 };
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
    fn typing_a_slash_lists_every_command_below_the_editor() {
        let lines = screen_with(100, "/", None, None, false);
        assert_eq!(lines.len(), 5 + COMMANDS.len());
        assert!(lines[4].starts_with("› /help") && lines[4].contains("mostra os comandos"), "{lines:?}");
        assert!(lines[5].starts_with("  /clear") && lines[6].starts_with("  /think"), "{lines:?}");
        assert!(lines[7].starts_with("  /exit"), "{lines:?}");
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
    fn history_walks_back_and_forward_and_restores_the_draft() {
        let mut h = History::default();
        assert_eq!(h.up("rascunho"), None, "sem histórico, nada muda");
        h.push("primeira");
        h.push("segunda");
        h.push("segunda");
        assert_eq!(h.items.len(), 2, "repetida em seguida não entra de novo");
        assert_eq!(h.up("digitando"), Some("segunda".into()));
        assert_eq!(h.up("segunda"), Some("primeira".into()));
        assert_eq!(h.up("primeira"), Some("primeira".into()), "para no começo");
        assert_eq!(h.down(), Some("segunda".into()));
        assert_eq!(h.down(), Some("digitando".into()), "volta ao rascunho");
        assert_eq!(h.down(), None, "fora do histórico, nada muda");
    }

    #[test]
    fn editing_a_recalled_item_starts_a_new_walk_from_the_end() {
        let mut h = History::default();
        h.push("a");
        h.push("b");
        assert_eq!(h.up(""), Some("b".into()));
        h.edited();
        assert_eq!(h.up("b mudado"), Some("b".into()));
        assert_eq!(h.down(), Some("b mudado".into()), "o texto editado vira o rascunho");
    }

    #[test]
    fn the_think_command_explains_its_cost_in_the_menu() {
        let lines = screen_with(120, "/th", None, None, false);
        assert!(lines[4].starts_with("› /think") && lines[4].contains("gasta mais contexto"), "{lines:?}");
    }

    #[test]
    fn footer_flags_thinking_only_when_it_is_on() {
        let off = screen_with(100, "", None, None, false).pop().unwrap();
        assert!(!off.contains("thinking"), "{off}");
        let mut editor = new_editor();
        editor.insert_str("");
        let mut cfg = settings(false);
        cfg.think = Arc::new(AtomicBool::new(true));
        let mut terminal = Terminal::new(TestBackend::new(100, viewport_height(1, 0))).unwrap();
        let view = View { editor: &editor, work: None, usage: None, settings: &cfg, menu: Vec::new(), menu_index: 0 };
        terminal.draw(|f| draw(f, &view)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..100).map(|x| buffer[(x, 4)].symbol().to_string()).collect();
        assert!(footer.starts_with("m:1b · Manual · thinking · contexto"), "{footer}");
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

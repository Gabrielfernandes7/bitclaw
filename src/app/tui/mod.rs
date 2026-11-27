//! Interface de terminal do BitClaw, com ratatui em tela cheia, como o Claude Code.
//!
//! Tela alternativa: o que estava no terminal (o `cargo run`, o shell) some ao abrir e volta ao sair.
//! A conversa, com o mascote e o cabeçalho como primeiras linhas, ocupa a parte de cima e rola com a
//! roda do mouse e com PageUp/PageDown; o editor, o rodapé e os painéis ficam fixos embaixo. Com o
//! mouse capturado o terminal não seleciona sozinho: arrastar na conversa seleciona, e soltar copia.
//!
//! O agente roda numa thread própria e fala com a interface por canais. Só este módulo escreve
//! na tela: a biblioteca continua sem imprimir nada.

pub(crate) mod brand;
mod clipboard;
mod commands;
mod history;
mod mention;
mod input;
mod print;
mod restart;
mod transcript;
mod markdown;
mod steps;
mod theme;
mod view;
mod worker;

use self::commands::{ARG_COMMANDS, COMMANDS, Cmd, ModelAction, Setting, parse_context, parse_model_arg, parse_temperature, temperature_effect};
use self::history::History;
use self::mention::Mention;
use self::view::*;
use self::worker::{Command, UiEvent, worker};
use self::theme::{GRAY, RED, YELLOW, bold, fg, palette};
use bitclaw::agent::ChatModel;
use bitclaw::models::{ModelCatalog, ModelInfo, Reasoning, SharedEffort};
use bitclaw::core::options::{Options, SharedOptions};
use bitclaw::context::Usage;
use bitclaw::session::Session;
use bitclaw::skills::{Disabled, Skill};
use bitclaw::tools::Approver;
use crate::app::skill_install::{self, Staged};
use self::transcript::{Point, Selection, Transcript};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyCode, KeyEvent,
    KeyEventKind, KeyModifiers, KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui_textarea::TextArea;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

// Máximo de linhas do editor. Passando disso, ele rola por dentro.
const MAX_EDITOR_ROWS: usize = 6;

pub struct Settings {
    pub title: String,
    // Linhas ao lado do mascote, abaixo do nome.
    pub info: Vec<String>,
    // Provedor e dados do modelo do início (ex.: "9.0B · nvfp4"), para a linha do modelo no cabeçalho.
    pub provider: String,
    pub details: String,
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
    // Contexto e temperatura em uso (mudam com `/context` e `/temp`) e os da configuração, para o `reset`.
    pub options: SharedOptions,
    pub default_options: Options,
    pub auto_yes: bool,
    pub color: bool,
    pub graphics: bool,
    // Captura o mouse: a roda rola a conversa e arrastar seleciona e copia. Sem ela (`--no-mouse`), o
    // terminal seleciona sozinho e a conversa rola só pelo teclado.
    pub mouse: bool,
    // Pasta do projeto, de onde vêm as sugestões de arquivo do `@`.
    pub root: std::path::PathBuf,
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
    // Passos do agente já terminados neste turno e tokens gastos nas chamadas ao modelo.
    steps: usize,
    tokens: u64,
    // O que o modelo está gerando na chamada em andamento. Some quando a chamada termina.
    thinking: String,
    answer: String,
    // Pedido de aprovação aguardando resposta e se ele é para executar um script.
    approval: Option<Sender<bool>>,
    approval_runs: bool,
    // A decisão pendente aceita `a` (aprovar todas as edições do turno): nem scripts nem remoções.
    approval_batchable: bool,
    // O usuário respondeu `a`: as próximas edições do turno passam, mostradas na conversa.
    approve_all: bool,
    cancelling: bool,
    // Tempo esperando o modelo: o que já fechou e, se ele responde agora, desde quando.
    model_done: Duration,
    model_since: Option<Instant>,
}

impl Work {
    fn new(activity: &str) -> Self {
        Self {
            started: Instant::now(),
            activity: activity.to_string(),
            steps: 0,
            tokens: 0,
            thinking: String::new(),
            answer: String::new(),
            approval: None,
            approval_runs: false,
            approval_batchable: false,
            approve_all: false,
            cancelling: false,
            model_done: Duration::ZERO,
            model_since: (activity == "pensando").then(Instant::now),
        }
    }

    // Troca a atividade e fecha ou abre a contagem do tempo de modelo.
    fn set_activity(&mut self, activity: String) {
        match (self.model_since, activity == "pensando") {
            (Some(since), false) => {
                self.model_done += since.elapsed();
                self.model_since = None;
            }
            (None, true) => self.model_since = Some(Instant::now()),
            _ => {}
        }
        self.activity = activity;
    }

    // Tempo total que o modelo levou neste turno, incluindo a resposta em andamento.
    fn model_time(&self) -> Duration {
        self.model_done + self.model_since.map_or(Duration::ZERO, |t| t.elapsed())
    }
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
    // Item escolhido no menu de comandos.
    menu_index: usize,
    // A conversa, a rolagem e a seleção.
    transcript: Transcript,
    // Onde a conversa foi desenhada da última vez, para saber o que o mouse clicou.
    chat_area: Rect,
    // Arrastando para selecionar: o botão foi apertado dentro da conversa e ainda não soltou.
    dragging: bool,
    // Aviso curto no rodapé (ex.: "copiado") e quando apareceu.
    flash: Option<(Instant, String)>,
    // A saudação do mascote está rodando no topo da conversa. Termina sozinha, ou na primeira tecla.
    greeting: bool,
    history: History,
    // Painel de skills aberto, se houver.
    picker: Option<Picker>,
    // Painel de modelos aberto, se houver.
    model_picker: Option<ModelPicker>,
    // Painel de esforço aberto, se houver.
    effort_panel: Option<EffortPanel>,
    // O número do mascote em andamento (a saudação de abertura).
    play: Option<(Instant, brand::Mood)>,
    // `Esc` fechou as sugestões de `@`; voltam ao digitar de novo.
    mention_off: bool,
    // Painel `/status`, se estiver aberto.
    status_panel: Option<StatusPanel>,
    // Turnos, passos e tempo da sessão, mostrados no painel `/status`.
    stats: SessionStats,
    // Chamadas seguidas da mesma ferramenta, ainda sem ir para a conversa (viram um bloco só).
    group: Option<steps::Group>,
    // `/restart-dev` compilou com sucesso: ao sair, reabre o binário novo no lugar deste.
    restart: bool,
    // `/skill install`: o download em andamento (numa thread) e a skill baixada que espera `yes` ou `no`.
    skill_job: Option<Receiver<Result<Staged, String>>>,
    skill_pending: Option<Staged>,
    // Protocolo gráfico do terminal para o mascote (iTerm2, Kitty, Sixel). None = fallback textual.
    graphics: Option<brand::Graphics>,
    mascot_proto: Option<ratatui_image::protocol::Protocol>,
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
        let options = Arc::clone(&settings.options);
        thread::spawn(move || {
            worker(make_session, make_model, auto_yes, command_rx, event_tx, cancel, active, effort, options)
        });
    }

    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    // Tela cheia na tela alternativa: o terminal de antes volta intacto ao sair.
    let terminal = ratatui::init();
    let mouse = settings.mouse;
    if mouse {
        let _ = execute!(io::stdout(), EnableMouseCapture);
    }
    let _ = execute!(io::stdout(), EnableBracketedPaste);
    if enhanced {
        let _ = execute!(io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES));
    }

    let graphics = if settings.graphics { brand::Graphics::detect() } else { None };
    let initial_mood = brand::opening_mood();
    let initial_proto = graphics.as_ref().and_then(|g| {
        let img = brand::frame_image(initial_mood, 0);
        g.new_protocol(img, ratatui::layout::Size::new(brand::WIDTH, view::MASCOT_ROWS)).ok()
    });

    let mut app = App {
        terminal,
        editor: new_editor(),
        work: None,
        usage: None,
        settings,
        commands,
        events,
        cancel,
        menu_index: 0,
        transcript: Transcript::default(),
        chat_area: Rect::default(),
        dragging: false,
        flash: None,
        greeting: false,
        history: History::default(),
        picker: None,
        model_picker: None, effort_panel: None,
        // Ao abrir, o mascote faz um dos números, sorteado a cada abertura.
        play: Some((Instant::now(), initial_mood)),
        mention_off: false,
        status_panel: None,
        stats: SessionStats::new(),
        group: None,
        restart: false,
        skill_job: None,
        skill_pending: None,
        graphics,
        mascot_proto: initial_proto,
    };
    let result = app.event_loop();

    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableBracketedPaste);
    if mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    ratatui::restore();
    if result.is_ok() && app.restart {
        return Err(restart::exec_self());
    }
    result
}

// Quanto tempo um aviso curto fica no rodapé.
const FLASH: Duration = Duration::from_millis(2_500);

fn new_editor() -> TextArea<'static> {
    let mut ta = TextArea::default();
    ta.set_cursor_line_style(Style::default());
    ta.set_placeholder_text("Digite uma tarefa…");
    ta.set_placeholder_style(fg(GRAY));
    ta
}

impl App {
    fn event_loop(&mut self) -> Result<(), String> {
        // A abertura é a primeira coisa da conversa. Com espaço para o mascote, ele anima ali mesmo.
        let width = self.terminal.size().map_err(|e| e.to_string())?.width;
        self.greeting = mascot_rect(Rect::new(0, 0, width, MASCOT_ROWS)).is_some();
        self.print_header().map_err(|e| e.to_string())?;
        loop {
            self.animate_greeting().map_err(|e| e.to_string())?;
            self.refresh_banner().map_err(|e| e.to_string())?;
            self.poll_skill_job().map_err(|e| e.to_string())?;
            if self.flash.as_ref().is_some_and(|(at, _)| at.elapsed() >= FLASH) {
                self.flash = None;
            }
            let size = self.terminal.size().map_err(|e| e.to_string())?;
            self.transcript.layout(size.width);
            let menu = self.menu_matches();
            let mentions = self.mention_matches().map(|m| m.entries).unwrap_or_default();
            // O que o modelo gera ao vivo fica embaixo, junto do editor, em até um terço da tela.
            let live_rows = view::live_lines(self.work.as_ref(), size.width).len().min(usize::from(size.height / 3));
            let App { terminal, editor, work, usage, settings, menu_index, picker, model_picker, effort_panel, status_panel, stats, transcript, flash, chat_area, mascot_proto, .. } = self;
            let view = View {
                editor,
                work: work.as_ref(),
                usage: *usage,
                settings,
                menu,
                menu_index: *menu_index,
                picker: picker.as_ref(),
                model_picker: model_picker.as_ref(),
                effort_panel: effort_panel.as_ref(),
                status_panel: status_panel.as_ref(),
                stats,
                transcript,
                flash: flash.as_ref().map(|(_, t)| t.as_str()),
                live_rows,
                mentions,
                mascot_image: mascot_proto.as_ref(),
            };
            terminal.draw(|f| *chat_area = draw(f, &view)).map_err(|e| e.to_string())?;
            self.transcript.clamp(usize::from(self.chat_area.height));
            if event::poll(Duration::from_millis(80)).map_err(|e| e.to_string())? {
                match event::read().map_err(|e| e.to_string())? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if self.on_key(key).map_err(|e| e.to_string())? {
                            return Ok(());
                        }
                    }
                    Event::Mouse(mouse) => self.on_mouse(mouse),
                    Event::Paste(text) if self.work.as_ref().is_none_or(|w| w.approval.is_none()) => {
                        self.editor.insert_str(text.replace("\r\n", "\n").replace('\r', "\n"));
                    }
                    // Redimensionar não precisa de nada: o próximo desenho usa a nova largura e reflui a conversa.
                    _ => {}
                }
            }
            self.drain_events()?;
        }
    }

    // Roda da mouse rola a conversa; arrastar nela seleciona e, ao soltar, copia.
    fn on_mouse(&mut self, ev: MouseEvent) {
        let chat = self.chat_area;
        let height = usize::from(chat.height);
        match ev.kind {
            MouseEventKind::ScrollUp => self.transcript.scroll(3, height),
            MouseEventKind::ScrollDown => self.transcript.scroll(-3, height),
            MouseEventKind::Down(MouseButton::Left) => {
                let inside = ev.row >= chat.y && ev.row < chat.y + chat.height && ev.column >= chat.x;
                self.transcript.selection = None;
                self.dragging = inside;
                if inside {
                    let p = self.point_at(ev.column, ev.row);
                    self.transcript.selection = Some(Selection { anchor: p, head: p });
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                // Arrastar além da borda rola a conversa, para selecionar mais do que cabe na tela.
                if ev.row < chat.y {
                    self.transcript.scroll(1, height);
                } else if ev.row >= chat.y + chat.height {
                    self.transcript.scroll(-1, height);
                }
                let row = ev.row.clamp(chat.y, (chat.y + chat.height).saturating_sub(1));
                let p = self.point_at(ev.column, row);
                if let Some(sel) = &mut self.transcript.selection {
                    sel.head = p;
                }
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
                self.copy_selection();
            }
            _ => {}
        }
    }

    // A posição na conversa sob a célula (`col`, `row`) da tela.
    fn point_at(&self, col: u16, row: u16) -> Point {
        let first = self.transcript.first_visible(usize::from(self.chat_area.height));
        Point { row: first + usize::from(row.saturating_sub(self.chat_area.y)), col: usize::from(col.saturating_sub(self.chat_area.x)) }
    }

    // Copia o que está selecionado e avisa no rodapé. Um clique sem arrastar só limpa a seleção.
    pub(super) fn copy_selection(&mut self) {
        match self.transcript.selected_text() {
            Some(text) => {
                let how = clipboard::copy(&text);
                self.flash = Some((Instant::now(), format!("{how} · {} caracteres", text.chars().count())));
            }
            None => self.transcript.selection = None,
        }
    }

    // Enquanto o mascote faz o número de abertura, a primeira linha da conversa acompanha o quadro.
    fn animate_greeting(&mut self) -> io::Result<()> {
        if !self.greeting {
            return Ok(());
        }
        if self.play.is_none_or(|(at, _)| at.elapsed().as_millis() >= brand::TRICK_MS) {
            return self.finish_greeting();
        }
        let width = self.terminal.size()?.width;
        let mascot = mascot_for(self.play);
        if let Some(g) = &self.graphics {
            let img = brand::frame_image(mascot.mood, mascot.ms);
            self.mascot_proto = g.new_protocol(img, ratatui::layout::Size::new(brand::WIDTH, view::MASCOT_ROWS)).ok();
        }
        let (banner, _) = view::banner_lines(&self.settings, &self.stats, mascot, width);
        self.transcript.replace_head(banner.into_iter().map(|l| print::clip_line(l, usize::from(width))).collect());
        Ok(())
    }

    // O cabeçalho acompanha o que muda na sessão (modelo, esforço, janela, tokens). Só reescreve se o
    // texto mudou. Durante a saudação, quem o redesenha é `animate_greeting`.
    fn refresh_banner(&mut self) -> io::Result<()> {
        if self.greeting {
            return Ok(());
        }
        let width = self.terminal.size()?.width;
        let (banner, _) = view::banner_lines(&self.settings, &self.stats, mascot_for(None), width);
        self.transcript.replace_head(banner.into_iter().map(|l| print::clip_line(l, usize::from(width))).collect());
        Ok(())
    }

    // Imprime o bloco de passos que estiver aberto, se houver, e o fecha.
    fn flush_group(&mut self) -> Result<(), String> {
        let Some(group) = self.group.take() else { return Ok(()) };
        let width = self.width().map_err(|e| e.to_string())?;
        self.print(group.lines(width)).map_err(|e| e.to_string())
    }

    // A saudação acabou (sozinha ou com uma tecla): o mascote para no primeiro quadro.
    pub(super) fn finish_greeting(&mut self) -> io::Result<()> {
        if !self.greeting {
            return Ok(());
        }
        self.greeting = false;
        self.play = None;
        if let Some(g) = &self.graphics {
            self.mascot_proto = g.new_protocol(brand::resting_image().clone(), ratatui::layout::Size::new(brand::WIDTH, view::MASCOT_ROWS)).ok();
        }
        let width = self.terminal.size()?.width;
        let (banner, _) = view::banner_lines(&self.settings, &self.stats, mascot_for(None), width);
        self.transcript.replace_head(banner.into_iter().map(|l| print::clip_line(l, usize::from(width))).collect());
        Ok(())
    }

    // Comandos que combinam com o que está sendo digitado. Só aparecem com o editor ocioso e
    // uma única palavra começando por `/`.
    fn menu_matches(&self) -> Vec<&'static Cmd> {
        let lines = self.editor.lines();
        match lines {
            // Comando recuperado do histórico: sem menu, para ↑ e ↓ seguirem no histórico.
            [line] if self.work.is_none() && !self.history.browsing() && line.starts_with('/') && !line.contains(char::is_whitespace) => {
                COMMANDS.iter().filter(|c| c.starts_with(line)).collect()
            }
            _ => Vec::new(),
        }
    }

    // `/clear`: apaga a conversa e recomeça com o cabeçalho, como no Claude Code.
    fn clear_screen(&mut self) -> io::Result<()> {
        self.finish_greeting()?;
        self.transcript.clear();
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
            // Um bloco de passos fecha quando chega algo que não é mais um passo da mesma sequência.
            if !matches!(event, UiEvent::Activity(_) | UiEvent::Chunk { .. } | UiEvent::Usage(_) | UiEvent::Step { .. }) {
                self.flush_group()?;
            }
            match event {
                UiEvent::Activity(a) => {
                    if let Some(w) = &mut self.work {
                        // A fase mudou: o que estava sendo gerado ao vivo já foi para a conversa ou acabou.
                        w.thinking.clear();
                        w.answer.clear();
                        if !w.cancelling {
                            w.set_activity(a);
                        }
                    }
                }
                UiEvent::Chunk { thinking, text } => {
                    if let Some(w) = &mut self.work {
                        if thinking { w.thinking.push_str(&text) } else { w.answer.push_str(&text) }
                    }
                }
                UiEvent::Usage(u) => self.usage = Some(u),
                UiEvent::Reasoning(text) => {
                    self.print_reasoning(&text).map_err(|e| e.to_string())?;
                }
                UiEvent::Tokens { prompt, output } => {
                    let sum = prompt.unwrap_or(0) + output.unwrap_or(0);
                    if let Some(w) = &mut self.work {
                        w.tokens += sum;
                    }
                    self.stats.tokens += sum;
                    if prompt.is_some() || output.is_some() {
                        let text = format!(
                            "↳ tokens: {} de entrada · {} de saída",
                            prompt.map_or("—".to_string(), |t| t.to_string()),
                            output.map_or("—".to_string(), |t| t.to_string()),
                        );
                        self.print(vec![note(&text)]).map_err(|e| e.to_string())?;
                    }
                }
                UiEvent::Step { name, args, result } => {
                    if let Some(w) = &mut self.work {
                        w.steps += 1;
                    }
                    self.stats.steps += 1;
                    match &mut self.group {
                        Some(g) if g.is(&name) => g.push(args, result),
                        _ => {
                            self.flush_group()?;
                            let mut g = steps::Group::new(&name);
                            g.push(args, result);
                            self.group = Some(g);
                        }
                    }
                }
                UiEvent::Approval { preview, reply, runs } => {
                    self.print_preview(&preview, runs).map_err(|e| e.to_string())?;
                    let batchable = !runs && preview.kind == bitclaw::tools::PreviewKind::Change;
                    match &mut self.work {
                        // `a` já foi dito neste turno: a edição passa, e a prévia acima mostra o que foi aplicado.
                        Some(w) if w.approve_all && batchable => {
                            let _ = reply.send(true);
                        }
                        Some(w) => {
                            w.approval = Some(reply);
                            w.approval_runs = runs;
                            w.approval_batchable = batchable;
                        }
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
    use bitclaw::models::{Effort, ModelCapabilities};
    use crate::app::tui::theme::KEYWORD;
    use ratatui::layout::Rect;
    use ratatui::style::Color;
    use bitclaw::skills;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn settings(auto_yes: bool) -> Settings {
        Settings {
            title: "BitClaw".into(),
            info: vec![],
            provider: "Ollama".into(),
            details: String::new(),
            skills: vec![],
            skills_off: skills::new_disabled(),
            model: Arc::new(Mutex::new(ModelInfo::unknown("m:1b"))),
            default_model: "m:1b".into(),
            catalog: Arc::new(StaticCatalog(Vec::new())),
            effort: Arc::new(Mutex::new(Effort::Off)),
            options: Arc::new(Mutex::new(Options::default())),
            default_options: Options::default(),
            auto_yes,
            color: false,
            graphics: false,
            mouse: true,
            root: std::env::temp_dir(),
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
        let summary = if work.is_some() { 1 } else { 0 };
        let height = viewport_height(editor.lines().len(), menu.len(), summary, 0);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let settings = settings(auto_yes);
        let view = View { editor: &editor, work, usage, settings: &settings, menu, menu_index: 0, picker: None, model_picker: None, effort_panel: None, status_panel: None, stats: &SessionStats::new(), transcript: &Transcript::default(), flash: None, live_rows: 0, mentions: Vec::new(), mascot_image: None };
        terminal.draw(|f| { draw(f, &view); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect()
    }

    fn screen(work: Option<&Work>, usage: Option<Usage>, auto_yes: bool) -> Vec<String> {
        screen_with(80, "", work, usage, auto_yes)
    }

    fn work(approval: bool, cancelling: bool) -> Work {
        let mut w = Work::new("executando read_file a.txt");
        w.approval = approval.then(|| mpsc::channel().0);
        w.cancelling = cancelling;
        w
    }

    #[test]
    fn idle_screen_is_compact_with_one_editor_row() {
        let usage = Usage { tokens: 861, window: 16384, percent: 5.0 };
        let lines = screen(None, Some(usage), false);
        assert_eq!(lines.len(), 4, "régua, 1 linha de editor, régua e rodapé: {lines:?}");
        assert!(lines.iter().all(|l| !l.contains("pensando") && !l.contains("executando")), "sem status quando ocioso");
        assert!(lines[0].starts_with('─') && lines[2].starts_with('─'));
        assert!(lines[1].starts_with('❯') && lines[1].contains("Digite uma tarefa…"), "{lines:?}");
        assert!(lines[3].starts_with("m:1b · contexto 5% (861/16384)"), "{}", lines[3]);
        assert!(lines[3].ends_with("enter envia · / comandos"), "versão curta dos atalhos em 80 colunas");
    }

    #[test]
    fn editor_grows_with_the_text_up_to_a_limit() {
        assert_eq!(screen_with(80, "a\nb\nc", None, None, false).len(), 6);
        assert_eq!(screen_with(80, &"x\n".repeat(20), None, None, false).len(), 4 + MAX_EDITOR_ROWS - 1);
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
        assert_eq!(lines.len(), 4 + COMMANDS.len());
        assert!(lines[3].starts_with("› /help") && lines[3].contains("mostra os comandos"), "{lines:?}");
        assert!(lines[4].starts_with("  /model") && lines[5].starts_with("  /effort"), "{lines:?}");
        assert!(lines[6].starts_with("  /context") && lines[7].starts_with("  /temp"), "{lines:?}");
        assert!(lines[8].starts_with("  /init"), "{lines:?}");
        assert!(lines[9].starts_with("  /clear") && lines[10].starts_with("  /think"), "{lines:?}");
        assert!(lines[11].starts_with("  /skills"), "{lines:?}");
        assert!(lines[12].starts_with("  /skill ") && lines[12].contains("baixa uma skill"), "{lines:?}");
        assert!(lines[13].starts_with("  /status"), "{lines:?}");
        assert!(lines[14].starts_with("  /undo"), "{lines:?}");
        assert!(lines[15].starts_with("  /restart-dev"), "{lines:?}");
        assert!(lines[16].starts_with("  /exit"), "{lines:?}");
        assert!(lines.last().unwrap().contains("tab completa · enter executa"));
    }

    #[test]
    fn the_menu_filters_by_name() {
        let lines = screen_with(100, "/cl", None, None, false);
        assert_eq!(lines.len(), 5);
        assert!(lines[3].starts_with("› /clear"), "{lines:?}");
        let lines = screen_with(100, "/ex", None, None, false);
        assert!(lines[3].starts_with("› /exit"), "{lines:?}");
        // Sem correspondência, não há menu.
        assert_eq!(screen_with(100, "/zzz", None, None, false).len(), 4);
    }

    #[test]
    fn the_think_command_explains_its_cost_in_the_menu() {
        let lines = screen_with(120, "/th", None, None, false);
        assert!(lines[3].starts_with("› /think") && lines[3].contains("gasta mais contexto"), "{lines:?}");
    }

    #[test]
    fn footer_flags_thinking_only_when_it_is_on() {
        let off = screen_with(100, "", None, None, false).pop().unwrap();
        assert!(!off.contains("esforço"), "{off}");
        let mut editor = new_editor();
        editor.insert_str("");
        let cfg = settings(false);
        *cfg.effort.lock().unwrap() = Effort::Medium;
        let mut terminal = Terminal::new(TestBackend::new(100, viewport_height(1, 0, 0, 0))).unwrap();
        let view = View { editor: &editor, work: None, usage: None, settings: &cfg, menu: Vec::new(), menu_index: 0, picker: None, model_picker: None, effort_panel: None, status_panel: None, stats: &SessionStats::new(), transcript: &Transcript::default(), flash: None, live_rows: 0, mentions: Vec::new(), mascot_image: None };
        terminal.draw(|f| { draw(f, &view); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..100).map(|x| buffer[(x, 3)].symbol().to_string()).collect();
        assert!(footer.starts_with("m:1b · esforço medium · contexto"), "{footer}");
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
        let mut terminal = Terminal::new(TestBackend::new(120, 7)).unwrap();
        terminal.draw(|f| draw_model_picker(f, f.area(), &cfg, &picker)).unwrap();
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
        assert!(lines[3].starts_with("m:1b · Automático · contexto —"), "{}", lines[3]);
    }

    #[test]
    fn working_screen_shows_the_activity_and_how_to_interrupt() {
        let lines = screen(Some(&work(false, false)), None, false);
        assert!(lines[0].contains("executando read_file a.txt… (0s)"), "{}", lines[0]);
        assert!(lines.last().unwrap().ends_with("esc interrompe"));
        let lines = screen(Some(&work(false, true)), None, false);
        assert!(lines[0].contains("interrompendo, aguardando o modelo"), "{}", lines[0]);
    }

    #[test]
    fn running_a_script_asks_in_its_own_words() {
        let mut w = work(true, false);
        w.approval_runs = true;
        let lines = screen(Some(&w), None, false);
        assert!(lines[2].contains("Executar este script?") && lines[2].contains("s  executar"), "{lines:?}");
        assert!(lines.iter().all(|l| !l.contains("[s] sim")), "uma só pergunta na tela: {lines:?}");
    }

    #[test]
    fn approval_screen_asks_for_a_decision() {
        let lines = screen(Some(&work(true, false)), None, false);
        assert!(lines[2].contains("Aplicar esta alteração?") && lines[2].contains("n  recusar"), "{lines:?}");
        assert!(lines.iter().all(|l| !l.contains("[s] sim") && !l.contains("s aplica")), "{lines:?}");
    }

    #[test]
    fn the_composer_becomes_the_question_while_waiting_for_a_decision() {
        let composer = |lines: &[String]| lines.iter().find(|l| l.starts_with('❯')).cloned().unwrap();
        let mut w = work(true, false);
        w.approval_batchable = true;
        let line = composer(&screen(Some(&w), None, false));
        assert!(line.contains("Aplicar esta alteração?"), "{line}");
        assert!(line.contains(" s  aplicar") && line.contains(" a  todas neste turno") && line.contains(" n  recusar"), "{line}");
        assert!(!line.contains("Digite uma tarefa"), "o placeholder some: {line}");
        // Estreito: as ações encurtam, mas as três teclas continuam à vista.
        let narrow = composer(&screen_with(50, "", Some(&w), None, false));
        assert!(narrow.contains(" s  sim") && narrow.contains(" a  todas") && narrow.ends_with(" n  não"), "{narrow}");
        // Sem decisão pendente, volta o editor.
        let line = composer(&screen(Some(&work(false, false)), None, false));
        assert!(line.contains("Digite uma tarefa"), "{line}");
    }

    #[test]
    fn an_edit_can_be_approved_for_the_whole_turn() {
        let mut w = work(true, false);
        w.approval_batchable = true;
        let lines = screen(Some(&w), None, false);
        assert!(lines[2].contains("a  todas neste turno"), "{lines:?}");
        assert!(lines.iter().all(|l| !l.contains("[a] sim")), "{lines:?}");
    }

    #[test]
    fn footer_drops_the_hints_when_the_window_is_narrow() {
        let footer = screen_with(40, "", None, None, false).pop().unwrap();
        assert!(footer.starts_with("m:1b · contexto —"));
        assert!(!footer.contains("enter envia"), "{footer}");
    }

    #[test]
    fn command_keywords_are_highlighted_in_the_editor() {
        let cell_colors = |typed: &str| -> Vec<Option<Color>> {
            let mut editor = new_editor();
            editor.insert_str(typed);
            let mut terminal = Terminal::new(TestBackend::new(60, viewport_height(1, 0, 0, 0))).unwrap();
            let settings = settings(false);
            let view = View { editor: &editor, work: None, usage: None, settings: &settings, menu: Vec::new(), menu_index: 0, picker: None, model_picker: None, effort_panel: None, status_panel: None, stats: &SessionStats::new(), transcript: &Transcript::default(), flash: None, live_rows: 0, mentions: Vec::new(), mascot_image: None };
            terminal.draw(|f| { draw(f, &view); }).unwrap();
            let buffer = terminal.backend().buffer().clone();
            // Linha do editor: logo depois da régua de cima → linha 1; o texto começa na coluna 2.
            (0..typed.chars().count() as u16).map(|x| buffer[(2 + x, 1)].fg).map(|c| (c != Color::Reset).then_some(c)).collect()
        };
        let colors = cell_colors("/model load x");
        assert!(colors[..6].iter().all(|c| *c == Some(KEYWORD)), "comando: {colors:?}");
        assert!(colors[7..11].iter().all(|c| *c == Some(KEYWORD)), "subcomando: {colors:?}");
        assert_eq!(colors[12], None, "o argumento fica normal");
        assert!(cell_colors("/efort").iter().all(Option::is_none), "comando desconhecido não é destacado");
        assert!(cell_colors("model").iter().all(Option::is_none), "sem a barra, nada é destacado");
    }

    #[test]
    fn the_mascot_rests_until_a_trick_is_played_and_goes_back_after_it() {
        assert_eq!(mascot_for(None), Mascot { mood: brand::Mood::Idle, ms: 0 });
        let playing = mascot_for(Some((Instant::now(), brand::Mood::Happy)));
        assert_eq!(playing.mood, brand::Mood::Happy);
        let old = Instant::now() - Duration::from_millis(brand::TRICK_MS as u64 + 100);
        assert_eq!(mascot_for(Some((old, brand::Mood::Happy))), Mascot { mood: brand::Mood::Idle, ms: 0 }, "o número acaba");
    }

    #[test]
    fn work_counts_the_time_spent_waiting_for_the_model() {
        let mut w = Work::new("pensando");
        assert!(w.model_since.is_some(), "começa esperando o modelo");
        w.model_since = Some(Instant::now() - Duration::from_secs(5));
        w.set_activity("executando read_file a.txt".into());
        assert!(w.model_since.is_none(), "rodar uma ferramenta fecha a contagem");
        assert!(w.model_time() >= Duration::from_secs(5));
        let closed = w.model_time();
        w.set_activity("executando list_dir".into());
        assert_eq!(w.model_time(), closed, "ferramenta não soma tempo de modelo");
        w.set_activity("pensando".into());
        assert!(w.model_since.is_some(), "voltar ao modelo reabre a contagem");
        assert!(Work::new("montando AGENTS.md").model_since.is_none());
    }

    #[test]
    fn the_step_line_sits_right_above_the_editor_while_working() {
        let busy = screen(Some(&work(false, false)), None, false);
        assert!(busy[0].contains("executando"), "o passo fica na linha de cima: {busy:?}");
        assert!(busy[1].starts_with('─') && busy[2].starts_with('❯'), "{busy:?}");
    }

    // Desenha a tela com o que o modelo está gerando, na altura dada e 60 colunas.
    fn render_live(live: &str, height: u16) -> Vec<String> {
        let editor = new_editor();
        let settings = settings(false);
        let mut w = Work::new("pensando");
        w.answer = live.to_string();
        let view = View {
            editor: &editor,
            work: (!live.is_empty()).then_some(&w),
            usage: None,
            settings: &settings,
            menu: Vec::new(),
            menu_index: 0,
            picker: None,
            model_picker: None,
            effort_panel: None,
            status_panel: None,
            stats: &SessionStats::new(),
            transcript: &Transcript::default(),
            flash: None,
            live_rows: if live.is_empty() { 0 } else { 3 },
            mentions: Vec::new(),
            mascot_image: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        terminal.draw(|f| { draw(f, &view); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect()
    }

    #[test]
    fn the_bottom_is_only_as_tall_as_the_editor_when_idle() {
        // Régua, editor, régua e rodapé.
        assert_eq!(viewport_height(1, 0, 0, 0), 4);
        assert_eq!(viewport_height(3, 2, 1, 0), 3 + 2 + 2 + 1 + 1);
        assert_eq!(viewport_height(99, 0, 0, 0), (MAX_EDITOR_ROWS + 3) as u16, "o editor para de crescer no limite");
        let lines = render_live("", viewport_height(1, 0, 0, 0));
        assert!(lines[0].starts_with('─') && lines[1].starts_with('❯') && lines[2].starts_with('─'), "{lines:?}");
    }

    // Desenha a tela inteira com uma conversa de `n` linhas, rolada `back` linhas para cima.
    fn render_chat(n: usize, back: usize, height: u16) -> Vec<String> {
        let editor = new_editor();
        let settings = settings(false);
        let mut transcript = Transcript::default();
        transcript.layout(60);
        transcript.push((0..n).map(|i| Line::from(format!("linha {i}"))).collect());
        transcript.scroll(back as isize, usize::from(height) - 4);
        let view = View {
            editor: &editor,
            work: None,
            usage: None,
            settings: &settings,
            menu: Vec::new(),
            menu_index: 0,
            picker: None,
            model_picker: None,
            effort_panel: None,
            status_panel: None,
            stats: &SessionStats::new(),
            transcript: &transcript,
            flash: None,
            live_rows: 0,
            mentions: Vec::new(),
            mascot_image: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        let mut chat = Rect::default();
        terminal.draw(|f| chat = draw(f, &view)).unwrap();
        assert_eq!(chat, Rect::new(0, 0, 60, height - 4), "a conversa fica com tudo acima do editor");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect()
    }

    #[test]
    fn the_conversation_fills_the_top_and_the_editor_stays_fixed_at_the_bottom() {
        let lines = render_chat(20, 0, 10);
        assert_eq!(lines[0], "linha 14", "acompanha o fim: {lines:?}");
        assert_eq!(lines[5], "linha 19", "{lines:?}");
        assert!(lines[6].starts_with('─') && lines[7].starts_with('❯'), "o editor logo abaixo: {lines:?}");
        // Pouca conversa: começa no topo, e o editor continua embaixo.
        let short = render_chat(2, 0, 10);
        assert_eq!((short[0].as_str(), short[1].as_str(), short[2].as_str()), ("linha 0", "linha 1", ""));
        assert!(short[7].starts_with('❯'), "{short:?}");
    }

    #[test]
    fn scrolled_up_it_says_how_to_go_back_to_the_end() {
        let lines = render_chat(20, 5, 10);
        assert_eq!(lines[0], "linha 9", "{lines:?}");
        assert!(lines[5].contains("↓ 5 linhas abaixo"), "{lines:?}");
        assert!(lines[6].starts_with('─'), "o editor não se move: {lines:?}");
    }

    #[test]
    fn a_panel_opens_below_the_conversation_without_hiding_it() {
        let editor = new_editor();
        let settings = settings(false);
        let mut transcript = Transcript::default();
        transcript.layout(80);
        transcript.push(vec![Line::from("a conversa continua visível")]);
        let panel = StatusPanel::default();
        let view = View {
            editor: &editor,
            work: None,
            usage: None,
            settings: &settings,
            menu: Vec::new(),
            menu_index: 0,
            picker: None,
            model_picker: None,
            effort_panel: None,
            status_panel: Some(&panel),
            stats: &SessionStats::new(),
            transcript: &transcript,
            flash: None,
            live_rows: 0,
            mentions: Vec::new(),
            mascot_image: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| {
            draw(f, &view);
        })
        .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..30)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect();
        assert_eq!(rows[0], "a conversa continua visível", "{rows:?}");
        let rule = 30 - STATUS_PANEL_ROWS as usize - 1;
        assert!(rows[rule].starts_with('─'), "uma régua separa o painel da conversa: {rows:?}");
        assert!(rows[rule + 1].contains("BitClaw"), "{rows:?}");
        assert_eq!(rows.iter().filter(|r| r.contains("██")).count(), 0, "nenhum mascote no painel: {rows:?}");
    }

    #[test]
    fn what_the_model_generates_sits_right_above_the_step_line() {
        let height = viewport_height(1, 0, 1, 3);
        let lines = render_live("primeira\nsegunda", height);
        let step = lines.iter().position(|l| l.contains("pensando")).expect("a linha do passo");
        assert_eq!(lines[step - 1], "  │ segunda", "{lines:?}");
        assert!(lines[step - 3].contains("respondendo"), "{lines:?}");
    }

    #[test]
    fn the_mascot_hides_in_narrow_windows() {
        let rect = mascot_rect(Rect::new(0, 7, 80, MASCOT_ROWS)).unwrap();
        assert_eq!((rect.width, rect.height), (brand::WIDTH, MASCOT_ROWS));
        assert_eq!(mascot_rect(Rect::new(0, 0, 20, MASCOT_ROWS)), None);
    }

    #[test]
    fn file_suggestions_appear_below_the_editor_with_their_own_hint() {
        let mut editor = new_editor();
        editor.insert_str("leia @do");
        let mentions = vec![
            mention::Entry { path: "docs/".into(), is_dir: true },
            mention::Entry { path: "README.md".into(), is_dir: false },
        ];
        let settings = settings(false);
        let view = View {
            editor: &editor,
            work: None,
            usage: None,
            settings: &settings,
            menu: Vec::new(),
            menu_index: 0,
            picker: None,
            model_picker: None, effort_panel: None,
            status_panel: None,
            stats: &SessionStats::new(),
            transcript: &Transcript::default(),
            flash: None,
            live_rows: 0,
            mentions,
            mascot_image: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
        terminal.draw(|f| { draw(f, &view); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..14)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect();
        let at = rows.iter().position(|r| r.starts_with("› @docs/")).unwrap_or_else(|| panic!("{rows:?}"));
        assert_eq!(rows[at + 1], "  @README.md", "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("tab ou enter completa")), "{rows:?}");
    }

    fn banner_text(settings: &Settings) -> String {
        let (lines, _) = view::banner_lines(settings, &SessionStats::new(), Mascot { mood: brand::Mood::Idle, ms: 0 }, 120);
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn the_header_follows_the_effort_the_model_and_the_window() {
        let mut s = settings(false);
        s.info = vec!["Projeto (p) /p".into()];
        s.details = "9.0B · nvfp4".into();
        let text = banner_text(&s);
        assert!(text.contains("Projeto (p) /p"), "{text}");
        assert!(text.contains("m:1b thinking off · Ollama · 9.0B · nvfp4 · janela 16384 tok"), "{text}");

        *locked(&s.effort) = Effort::High;
        assert!(banner_text(&s).contains("m:1b with high effort · Ollama"), "o esforço novo aparece sem reabrir");

        locked(&s.model).id = "outro:3b".into();
        let text = banner_text(&s);
        assert!(text.contains("outro:3b with high effort · Ollama · janela"), "{text}");
        assert!(!text.contains("nvfp4"), "os dados do modelo antigo não ficam: {text}");

        locked(&s.options).num_ctx = 32768;
        assert!(banner_text(&s).contains("janela 32768 tok"));
    }
    // O mascote sai inteiro (3 linhas × 10 colunas) em vários tamanhos de terminal, com o texto começando
    // depois dele; abaixo de 24 colunas ele some e o texto fica. Desenha a tela toda, como no programa.
    #[test]
    fn the_mascot_is_drawn_whole_at_every_terminal_size() {
        let mut settings = settings(false);
        settings.info = vec!["Projeto (ClawBit) /Users/alguem/Documents/Pessoal/ClawBit".into()];
        settings.details = "4.0B · nvfp4".into();
        let crab = brand::crab();
        for (w, h) in [(80u16, 24u16), (100, 30), (120, 40), (40, 12), (30, 10), (24, 8), (23, 8)] {
            let mut t = Transcript::default();
            let (banner, drawn) = banner_lines(&settings, &SessionStats::new(), mascot_for(None), w);
            t.push(banner.into_iter().map(|l| print::clip_line(l, usize::from(w))).collect());
            t.layout(w);
            let editor = new_editor();
            let view = View { editor: &editor, work: None, usage: None, settings: &settings, menu: vec![], menu_index: 0, picker: None, model_picker: None, effort_panel: None, status_panel: None, stats: &SessionStats::new(), transcript: &t, flash: None, live_rows: 0, mentions: Vec::new(), mascot_image: None };
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| { draw(f, &view); }).unwrap();
            let b = terminal.backend().buffer().clone();
            let row = |y: u16| (0..w).map(|x| b[(x, y)].symbol().to_string()).collect::<String>();
            if w < brand::WIDTH + 2 + 12 {
                assert!(!drawn && !(0..h).any(|y| row(y).contains('█')), "{w}x{h}: sem espaço, sem mascote");
                continue;
            }
            assert!(drawn, "{w}x{h}");
            for (i, art) in crab.iter().enumerate() {
                let line = row(TOP_GAP + i as u16);
                let (glyph, rest): (String, String) = (line.chars().take(10).collect(), line.chars().skip(10).collect());
                assert_eq!(&glyph, art, "{w}x{h}, linha {i} do mascote: {line:?}");
                assert!(rest.trim().is_empty() || (rest.starts_with("  ") && !rest[2..].starts_with(' ')), "{w}x{h}: texto logo depois do mascote: {line:?}");
            }
            assert!(row(TOP_GAP).contains("BitClaw") && row(TOP_GAP + 2).contains("m:1b"), "{w}x{h}");
        }
    }

    #[test]
    fn graphical_banner_uses_spacer_and_preserves_text_alignment() {
        let mut settings = settings(false);
        settings.graphics = true;
        settings.info = vec!["Projeto (p) /p".into()];
        let (lines, drawn) = banner_lines(&settings, &SessionStats::new(), mascot_for(None), 80);
        assert!(drawn);
        assert_eq!(lines.len(), BANNER_ROWS);
        for i in 0..MASCOT_ROWS as usize {
            let row = &lines[TOP_GAP as usize + i];
            assert_eq!(row.spans[0].content, "          ", "10 espaços reservados para a imagem do mascote");
            assert_eq!(row.spans[1].content, "  ", "2 espaços de margem entre mascote e texto");
        }
        let full = lines[TOP_GAP as usize].spans.iter().map(|s| s.content.as_ref()).collect::<String>();
        assert!(full.contains("BitClaw"));
    }

    #[test]
    fn graphical_mascot_renders_image_widget_when_active() {
        let mut settings = settings(false);
        settings.graphics = true;
        let mut t = Transcript::default();
        let (banner, _) = banner_lines(&settings, &SessionStats::new(), mascot_for(None), 80);
        t.push(banner.into_iter().map(|l| print::clip_line(l, 80)).collect());
        t.layout(80);

        let g = brand::Graphics::with_protocol(ratatui_image::picker::ProtocolType::Iterm2);
        let proto = g.new_protocol(brand::resting_image().clone(), ratatui::layout::Size::new(brand::WIDTH, MASCOT_ROWS)).unwrap();

        let editor = new_editor();
        let view = View {
            editor: &editor,
            work: None,
            usage: None,
            settings: &settings,
            menu: vec![],
            menu_index: 0,
            picker: None,
            model_picker: None,
            effort_panel: None,
            status_panel: None,
            stats: &SessionStats::new(),
            transcript: &t,
            flash: None,
            live_rows: 0,
            mentions: Vec::new(),
            mascot_image: Some(&proto),
        };

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| { draw(f, &view); }).unwrap();
        let buffer = terminal.backend().buffer().clone();
        // A célula do canto superior do mascote (coluna 0, linha TOP_GAP) recebe o símbolo do protocolo iTerm2 (escape OSC 1337)
        let top_left = &buffer[(0, TOP_GAP)];
        assert!(top_left.symbol().contains("1337;File=inline=1"), "contém a sequência iTerm2: {:?}", top_left.symbol());
    }
}

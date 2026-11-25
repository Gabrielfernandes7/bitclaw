//! Interface de terminal do BitClaw, com ratatui em tela cheia.
//!
//! Tela cheia: o mascote com o nome e o status ficam no topo, a conversa no meio (rola com
//! PageUp/PageDown e a roda do mouse) e o editor com o rodapé embaixo.
//!
//! O agente roda numa thread própria e fala com a interface por canais. Só este módulo escreve
//! na tela: a biblioteca continua sem imprimir nada.

mod brand;
mod commands;
mod history;
mod mention;
mod input;
mod print;
mod markdown;
pub mod plain;
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
use bitclaw::models::{Effort, ModelCatalog, ModelInfo, Reasoning, SharedEffort};
use bitclaw::ollama::{Options, SharedOptions};
use bitclaw::context::Usage;
use bitclaw::session::Session;
use bitclaw::skills::{Disabled, Skill};
use bitclaw::tools::Approver;
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyCode,
    KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::DefaultTerminal;
use ratatui_textarea::TextArea;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

// Máximo de linhas do editor. Passando disso, ele rola por dentro.
const MAX_EDITOR_ROWS: usize = 6;

// Máximo de linhas guardadas na conversa. As mais antigas saem primeiro.
const MAX_TRANSCRIPT: usize = 20_000;

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
    // Contexto e temperatura em uso (mudam com `/context` e `/temp`) e os da configuração, para o `reset`.
    pub options: SharedOptions,
    pub default_options: Options,
    pub auto_yes: bool,
    pub color: bool,
    // Captura o mouse, para a roda rolar a conversa e o clique no mascote. Com ela ligada, o terminal
    // não deixa selecionar texto; por isso vem desligada e liga-se com `--mouse`.
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
    // A conversa, mostrada entre o cabeçalho e o editor, e quantas linhas ela está rolada para cima
    // (0 acompanha o fim).
    transcript: Vec<Line<'static>>,
    scroll_back: usize,
    history: History,
    // Painel de skills aberto, se houver.
    picker: Option<Picker>,
    // Painel de modelos aberto, se houver.
    model_picker: Option<ModelPicker>,
    // Número do mascote em andamento e quantos cliques já houve (decide o próximo número).
    play: Option<(Instant, brand::Mood)>,
    clicks: usize,
    // `Esc` fechou as sugestões de `@`; voltam ao digitar de novo.
    mention_off: bool,
    // Painel `/status`, se estiver aberto.
    status_panel: Option<StatusPanel>,
    // Turnos, passos e tempo da sessão, mostrados no painel `/status`.
    stats: SessionStats,
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
    // Tela cheia: a conversa fica entre o cabeçalho e o editor.
    let terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableBracketedPaste);
    if settings.mouse {
        let _ = execute!(io::stdout(), EnableMouseCapture);
    }
    let mouse = settings.mouse;
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
        menu_index: 0,
        transcript: Vec::new(),
        scroll_back: 0,
        history: History::default(),
        picker: None,
        model_picker: None,
        // Ao abrir, o mascote faz um dos números, sorteado a cada abertura.
        play: Some((Instant::now(), brand::opening_mood())),
        clicks: 0,
        mention_off: false,
        status_panel: None,
        stats: SessionStats::new(),
    };
    let result = app.event_loop();

    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    if mouse {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
    let _ = execute!(io::stdout(), DisableBracketedPaste);
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
            let menu = self.menu_matches();
            let mentions = self.mention_matches().map(|m| m.entries).unwrap_or_default();
            let App { terminal, editor, work, usage, settings, menu_index, picker, model_picker, play, transcript, scroll_back, status_panel, stats, .. } = self;
            let mascot = mascot_for(*play);
            let view = View {
                editor,
                work: work.as_ref(),
                usage: *usage,
                settings,
                menu,
                menu_index: *menu_index,
                picker: picker.as_ref(),
                model_picker: model_picker.as_ref(),
                status_panel: status_panel.as_ref(),
                stats,
                mascot,
                transcript,
                scroll_back: *scroll_back,
                mentions,
            };
            terminal.draw(|f| draw(f, &view)).map_err(|e| e.to_string())?;
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
                    _ => {}
                }
            }
            self.drain_events()?;
        }
    }

    // Comandos que combinam com o que está sendo digitado. Só aparecem com o editor ocioso e
    // uma única palavra começando por `/`.
    // Clique no mascote: dispara o próximo número. Ignora cliques com um painel aberto.
    fn on_mouse(&mut self, event: MouseEvent) {
        if self.picker.is_some() || self.model_picker.is_some() || self.status_panel.is_some() {
            return;
        }
        match event.kind {
            MouseEventKind::ScrollUp => return self.scroll_by(3),
            MouseEventKind::ScrollDown => return self.scroll_by(-3),
            MouseEventKind::Down(MouseButton::Left) => {}
            _ => return,
        }
        let area = self.terminal.get_frame().area();
        let Some(rect) = mascot_rect(Rect::new(area.x, area.y + TOP_GAP, area.width, MASCOT_ROWS)) else { return };
        if rect.contains(Position::new(event.column, event.row)) {
            let mood = brand::TRICKS[self.clicks % brand::TRICKS.len()];
            self.clicks += 1;
            self.play = Some((Instant::now(), mood));
        }
    }

    fn menu_matches(&self) -> Vec<&'static Cmd> {
        let lines = self.editor.lines();
        match lines {
            [line] if self.work.is_none() && line.starts_with('/') && !line.contains(char::is_whitespace) => {
                COMMANDS.iter().filter(|c| c.starts_with(line)).collect()
            }
            _ => Vec::new(),
        }
    }

    fn clear_screen(&mut self) -> io::Result<()> {
        self.transcript.clear();
        self.scroll_back = 0;
        self.terminal.clear()?;
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
                    let width = self.width().map_err(|e| e.to_string())?;
                    let mut lines = vec![steps::done(&name, &args, &result)];
                    lines.extend(steps::output_lines(&result, width));
                    self.print(lines).map_err(|e| e.to_string())?;
                }
                UiEvent::Approval { preview, reply, runs } => {
                    self.print_preview(&preview, runs).map_err(|e| e.to_string())?;
                    match &mut self.work {
                        Some(w) => {
                            w.approval = Some(reply);
                            w.approval_runs = runs;
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
    use bitclaw::models::ModelCapabilities;
    use crate::tui::theme::KEYWORD;
    use ratatui::layout::{Position, Rect};
    use ratatui::style::Color;
    use bitclaw::skills;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    // Linhas que as partes fixas pedem (cabeçalho, réguas, editor, menu e rodapé), mais algumas
    // para a conversa.
    const TRANSCRIPT_ROWS: usize = 2;

    fn viewport_height(editor_rows: usize, menu_rows: usize, summary_rows: usize) -> u16 {
        (TOP_GAP as usize + MASCOT_ROWS as usize + summary_rows + TRANSCRIPT_ROWS + 2 + editor_rows.clamp(1, MAX_EDITOR_ROWS) + menu_rows + 1) as u16
    }

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
            options: Arc::new(Mutex::new(Options::default())),
            default_options: Options::default(),
            auto_yes,
            color: false,
            mouse: false,
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
        let summary = if work.is_some() { 2 } else { 0 };
        let height = viewport_height(editor.lines().len(), menu.len(), summary);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let settings = settings(auto_yes);
        let view = View { editor: &editor, work, usage, settings: &settings, menu, menu_index: 0, picker: None, model_picker: None, status_panel: None, stats: &SessionStats::new(), mascot: Mascot { mood: brand::Mood::Idle, ms: 0 }, transcript: &[], scroll_back: 0, mentions: Vec::new() };
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
        let mut w = Work::new("executando read_file a.txt");
        w.approval = approval.then(|| mpsc::channel().0);
        w.cancelling = cancelling;
        w
    }

    #[test]
    fn idle_screen_is_compact_with_one_editor_row() {
        let usage = Usage { tokens: 861, window: 16384, percent: 5.0 };
        let lines = screen(None, Some(usage), false);
        assert_eq!(lines.len(), 10, "folga (1), mascote (3), conversa (2), régua, 1 linha de editor, régua e rodapé: {lines:?}");
        assert!(lines[1..4].iter().all(|l| !l.contains("pensando") && !l.contains("executando")), "sem status quando ocioso");
        assert!(lines[0].is_empty(), "a folga do topo fica em branco: {lines:?}");
        assert!(lines[1].contains('▀') || lines[2].contains('█'), "o mascote aparece: {lines:?}");
        assert!(lines[6].starts_with('─') && lines[8].starts_with('─'));
        assert!(lines[7].starts_with('❯') && lines[7].contains("Digite uma tarefa…"), "{lines:?}");
        assert!(lines[9].starts_with("m:1b · Manual · contexto 5% (861/16384)"), "{}", lines[9]);
        assert!(lines[9].ends_with("enter envia · / comandos"), "versão curta dos atalhos em 80 colunas");
    }

    #[test]
    fn editor_grows_with_the_text_up_to_a_limit() {
        assert_eq!(screen_with(80, "a\nb\nc", None, None, false).len(), 12);
        assert_eq!(screen_with(80, &"x\n".repeat(20), None, None, false).len(), 10 + MAX_EDITOR_ROWS - 1);
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
        assert_eq!(lines.len(), 10 + COMMANDS.len());
        assert!(lines[9].starts_with("› /help") && lines[9].contains("mostra os comandos"), "{lines:?}");
        assert!(lines[10].starts_with("  /model") && lines[11].starts_with("  /effort"), "{lines:?}");
        assert!(lines[12].starts_with("  /context") && lines[13].starts_with("  /temp"), "{lines:?}");
        assert!(lines[14].starts_with("  /init"), "{lines:?}");
        assert!(lines[15].starts_with("  /clear") && lines[16].starts_with("  /think"), "{lines:?}");
        assert!(lines[17].starts_with("  /skills"), "{lines:?}");
        assert!(lines[18].starts_with("  /status"), "{lines:?}");
        assert!(lines[19].starts_with("  /exit"), "{lines:?}");
        assert!(lines.last().unwrap().contains("tab completa · enter executa"));
    }

    #[test]
    fn the_menu_filters_by_name() {
        let lines = screen_with(100, "/cl", None, None, false);
        assert_eq!(lines.len(), 11);
        assert!(lines[9].starts_with("› /clear"), "{lines:?}");
        let lines = screen_with(100, "/ex", None, None, false);
        assert!(lines[9].starts_with("› /exit"), "{lines:?}");
        // Sem correspondência, não há menu.
        assert_eq!(screen_with(100, "/zzz", None, None, false).len(), 10);
    }

    #[test]
    fn the_think_command_explains_its_cost_in_the_menu() {
        let lines = screen_with(120, "/th", None, None, false);
        assert!(lines[9].starts_with("› /think") && lines[9].contains("gasta mais contexto"), "{lines:?}");
    }

    #[test]
    fn footer_flags_thinking_only_when_it_is_on() {
        let off = screen_with(100, "", None, None, false).pop().unwrap();
        assert!(!off.contains("esforço"), "{off}");
        let mut editor = new_editor();
        editor.insert_str("");
        let cfg = settings(false);
        *cfg.effort.lock().unwrap() = Effort::Medium;
        let mut terminal = Terminal::new(TestBackend::new(100, viewport_height(1, 0, 0))).unwrap();
        let view = View { editor: &editor, work: None, usage: None, settings: &cfg, menu: Vec::new(), menu_index: 0, picker: None, model_picker: None, status_panel: None, stats: &SessionStats::new(), mascot: Mascot { mood: brand::Mood::Idle, ms: 0 }, transcript: &[], scroll_back: 0, mentions: Vec::new() };
        terminal.draw(|f| draw(f, &view)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..100).map(|x| buffer[(x, 9)].symbol().to_string()).collect();
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
        let mut terminal = Terminal::new(TestBackend::new(120, 7)).unwrap();
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
        assert!(lines[9].starts_with("m:1b · Automático · contexto —"), "{}", lines[9]);
    }

    #[test]
    fn working_screen_shows_the_activity_and_how_to_interrupt() {
        let lines = screen(Some(&work(false, false)), None, false);
        assert!(lines[5].contains("executando read_file a.txt… (0s)"), "{}", lines[1]);
        assert!(lines.last().unwrap().ends_with("esc interrompe"));
        let lines = screen(Some(&work(false, true)), None, false);
        assert!(lines[5].contains("interrompendo, aguardando o modelo"), "{}", lines[1]);
    }

    #[test]
    fn running_a_script_asks_in_its_own_words() {
        let mut w = work(true, false);
        w.approval_runs = true;
        let lines = screen(Some(&w), None, false);
        assert!(lines[5].contains("Executar este script? [s] sim · [n] não"), "{lines:?}");
        assert!(lines.last().unwrap().ends_with("s executa · n recusa"), "{lines:?}");
    }

    #[test]
    fn approval_screen_asks_for_a_decision() {
        let lines = screen(Some(&work(true, false)), None, false);
        assert!(lines[5].contains("Aplicar esta alteração? [s] sim · [n] não"), "{}", lines[1]);
        assert!(lines.last().unwrap().ends_with("s aplica · n recusa"));
    }

    #[test]
    fn footer_drops_the_hints_when_the_window_is_narrow() {
        let footer = screen_with(40, "", None, None, false).pop().unwrap();
        assert!(footer.starts_with("m:1b · Manual · contexto —"));
        assert!(!footer.contains("enter envia"), "{footer}");
    }

    #[test]
    fn command_keywords_are_highlighted_in_the_editor() {
        let cell_colors = |typed: &str| -> Vec<Option<Color>> {
            let mut editor = new_editor();
            editor.insert_str(typed);
            let mut terminal = Terminal::new(TestBackend::new(60, viewport_height(1, 0, 0))).unwrap();
            let settings = settings(false);
            let view = View { editor: &editor, work: None, usage: None, settings: &settings, menu: Vec::new(), menu_index: 0, picker: None, model_picker: None, status_panel: None, stats: &SessionStats::new(), mascot: Mascot { mood: brand::Mood::Idle, ms: 0 }, transcript: &[], scroll_back: 0, mentions: Vec::new() };
            terminal.draw(|f| draw(f, &view)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            // Linha do editor: folga (1) + cabeçalho (3) + conversa (2) + régua (1) → linha 7; o texto começa na coluna 2.
            (0..typed.chars().count() as u16).map(|x| buffer[(2 + x, 7)].fg).map(|c| (c != Color::Reset).then_some(c)).collect()
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
    fn only_the_mascot_area_is_clickable_and_it_hides_in_narrow_windows() {
        let status = Rect::new(0, 7, 80, MASCOT_ROWS);
        let rect = mascot_rect(status).unwrap();
        assert!(rect.contains(Position::new(0, 7)) && rect.contains(Position::new(9, 9)));
        assert!(!rect.contains(Position::new(10, 7)) && !rect.contains(Position::new(0, 10)));
        assert_eq!(mascot_rect(Rect::new(0, 0, 20, MASCOT_ROWS)), None);
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
    fn the_name_stays_beside_the_mascot_and_the_step_summary_sits_under_it() {
        let idle = screen(None, None, false);
        assert!(idle[1].starts_with("██") && idle[1].contains("BitClaw"), "{idle:?}");
        let busy = screen(Some(&work(false, false)), None, false);
        assert!(busy[1].contains("BitClaw"), "o nome continua no topo: {busy:?}");
        assert!(busy[4].is_empty(), "linha em branco antes do resumo: {busy:?}");
        assert!(busy[5].contains("executando"), "o resumo do passo tem linha própria: {busy:?}");
    }

    // Desenha a tela inteira com a conversa dada e `back` linhas de rolagem.
    fn screen_with_transcript(height: u16, lines: &[&str], back: usize) -> Vec<String> {
        let transcript: Vec<Line<'static>> = lines.iter().map(|l| Line::from(l.to_string())).collect();
        let editor = new_editor();
        let settings = settings(false);
        let view = View {
            editor: &editor,
            work: None,
            usage: None,
            settings: &settings,
            menu: Vec::new(),
            menu_index: 0,
            picker: None,
            model_picker: None,
            status_panel: None,
            stats: &SessionStats::new(),
            mascot: Mascot { mood: brand::Mood::Idle, ms: 0 },
            transcript: &transcript,
            scroll_back: back,
            mentions: Vec::new(),
        };
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        terminal.draw(|f| draw(f, &view)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect()
    }

    #[test]
    fn the_conversation_sits_between_the_header_and_the_prompt() {
        let lines = screen_with_transcript(12, &["primeira", "segunda"], 0);
        assert!(lines[1].contains("BitClaw"), "cabeçalho no topo: {lines:?}");
        assert_eq!(lines[4], "primeira", "{lines:?}");
        assert_eq!(lines[5], "segunda", "{lines:?}");
        let ruler = lines.iter().position(|l| l.starts_with('─')).unwrap();
        assert!(ruler > 4, "o prompt vem depois da conversa: {lines:?}");
        assert!(lines[ruler + 1].starts_with('❯'));
    }

    #[test]
    fn a_long_conversation_follows_the_end_and_can_be_scrolled_back() {
        let all: Vec<String> = (1..=30).map(|n| format!("linha {n}")).collect();
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        // 13 linhas de tela: 1 de folga, 3 de cabeçalho, 4 de partes fixas (régua, editor, régua e rodapé) e 5 para a conversa.
        let at_end = screen_with_transcript(13, &refs, 0);
        assert_eq!(&at_end[4..9], ["linha 26", "linha 27", "linha 28", "linha 29", "linha 30"]);
        let back = screen_with_transcript(13, &refs, 10);
        assert_eq!(&back[4..9], ["linha 16", "linha 17", "linha 18", "linha 19", "linha 20"]);
        let too_far = screen_with_transcript(13, &refs, 500);
        assert_eq!(&too_far[4..9], ["linha 1", "linha 2", "linha 3", "linha 4", "linha 5"], "para no começo");
    }

    #[test]
    fn visible_range_clamps_to_the_conversation() {
        assert_eq!(visible_range(0, 5, 0), (0, 0));
        assert_eq!(visible_range(3, 5, 0), (0, 3), "cabe inteira");
        assert_eq!(visible_range(10, 4, 0), (6, 10));
        assert_eq!(visible_range(10, 4, 3), (3, 7));
        assert_eq!(visible_range(10, 4, 99), (0, 4));
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
            model_picker: None,
            status_panel: None,
            stats: &SessionStats::new(),
            mascot: Mascot { mood: brand::Mood::Idle, ms: 0 },
            transcript: &[],
            scroll_back: 0,
            mentions,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
        terminal.draw(|f| draw(f, &view)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..14)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect();
        let at = rows.iter().position(|r| r.starts_with("› @docs/")).unwrap_or_else(|| panic!("{rows:?}"));
        assert_eq!(rows[at + 1], "  @README.md", "{rows:?}");
        assert!(rows.last().unwrap().contains("tab ou enter completa"), "{rows:?}");
    }
}

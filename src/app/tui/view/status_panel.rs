//! Painel de status (`/status`): versão, modelo, contexto e estatísticas da sessão, em abas.

use super::super::brand::{self, Mood};
use super::super::{Settings, theme::*};
use super::MASCOT_ROWS;
use bitclaw::context::Usage;
use bitclaw::skills;
use bitclaw::tools::BUILTIN_TOOLS;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

// Nomes das abas, na ordem em que aparecem.
pub(in crate::tui) const TABS: [&str; 4] = ["Status", "Config", "Usage", "Stats"];

// Largura da barra de contexto, em células.
const BAR_CELLS: usize = 20;

// Aba escolhida. Começa em Status.
#[derive(Default)]
pub(in crate::tui) struct StatusPanel {
    pub(in crate::tui) tab: usize,
}

impl StatusPanel {
    // Próxima aba, dando a volta no fim. `back` vai para a anterior.
    pub(in crate::tui) fn step(&mut self, back: bool) {
        self.tab = if back { (self.tab + TABS.len() - 1) % TABS.len() } else { (self.tab + 1) % TABS.len() };
    }
}

// Contagens da sessão: turnos enviados, passos do agente e o instante em que a sessão começou.
pub(in crate::tui) struct SessionStats {
    pub(in crate::tui) turns: usize,
    pub(in crate::tui) steps: usize,
    pub(in crate::tui) started: Instant,
    // Tokens de entrada e de saída somados em todas as chamadas ao modelo da sessão.
    pub(in crate::tui) tokens: u64,
}

impl SessionStats {
    pub(in crate::tui) fn new() -> Self {
        Self { turns: 0, steps: 0, started: Instant::now(), tokens: 0 }
    }
}

// O que o painel mostra, já lido do estado da interface.
pub(in crate::tui) struct Snapshot<'a> {
    pub(in crate::tui) settings: &'a Settings,
    pub(in crate::tui) model: String,
    pub(in crate::tui) effort: String,
    pub(in crate::tui) usage: Option<Usage>,
    pub(in crate::tui) stats: &'a SessionStats,
}

// Barra de contexto: `█` até a porcentagem, `░` o resto.
fn bar(percent: f64) -> String {
    let filled = ((percent.clamp(0.0, 100.0) / 100.0) * BAR_CELLS as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(BAR_CELLS - filled))
}

// Duração como `12min 03s`.
fn elapsed(secs: u64) -> String {
    format!("{}min {:02}s", secs / 60, secs % 60)
}

// Pares rótulo e valor da aba escolhida.
pub(in crate::tui) fn rows(tab: usize, s: &Snapshot) -> Vec<(&'static str, String)> {
    let st = s.settings;
    let off = skills::disabled_now(&st.skills_off);
    let on = st.skills.iter().filter(|k| !off.contains(&k.name)).count();
    let mode = if st.auto_yes { "automático" } else { "manual" };
    let options = *super::super::locked(&st.options);
    match tab {
        0 => {
            let model = if s.model == st.default_model {
                s.model.clone()
            } else {
                format!("{} · padrão {}", s.model, st.default_model)
            };
            vec![
                ("Versão", env!("CARGO_PKG_VERSION").to_string()),
                ("Projeto", st.root.display().to_string()),
                ("Modelo", model),
                ("Esforço", s.effort.clone()),
                ("Modo", mode.to_string()),
                ("Skills", format!("{on} ativas de {} carregadas", st.skills.len())),
                ("Ferramentas", BUILTIN_TOOLS.join(", ")),
            ]
        }
        1 => vec![
            ("Modelo padrão", st.default_model.clone()),
            ("Contexto", format!("{} tokens (padrão {})", options.num_ctx, st.default_options.num_ctx)),
            ("Temperatura", format!("{} (padrão {})", options.temperature, st.default_options.temperature)),
            ("Modo", mode.to_string()),
            ("Cores", if st.color { "ligadas" } else { "desligadas" }.to_string()),
            ("Mouse", if st.mouse { "ligado" } else { "desligado" }.to_string()),
            ("Skills desligadas", (st.skills.len() - on).to_string()),
        ],
        2 => match s.usage {
            Some(u) => vec![
                ("Contexto", format!("{} {:.0}%", bar(u.percent), u.percent)),
                ("Tokens", format!("{} de {}", u.tokens, u.window)),
            ],
            None => vec![("Contexto", "ainda não há uso: envie uma tarefa".to_string())],
        },
        _ => vec![
            ("Tempo da sessão", elapsed(s.stats.started.elapsed().as_secs())),
            ("Turnos", s.stats.turns.to_string()),
            ("Passos do agente", s.stats.steps.to_string()),
            ("Tokens gastos", s.stats.tokens.to_string()),
        ],
    }
}

// Desenha o painel inteiro: cabeçalho com o mascote, abas e o conteúdo da aba.
pub(in crate::tui) fn draw_status_panel(f: &mut Frame, s: &Snapshot, panel: &StatusPanel) {
    let [head, _, tabs, _, body, hint] = Layout::vertical([
        Constraint::Length(MASCOT_ROWS),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(f.area());

    // Mascote à esquerda e o nome com a versão ao lado, como no cabeçalho da conversa.
    let mascot: Vec<Line> = brand::frame(Mood::Idle, 0).into_iter().map(|l| Line::from(Span::styled(l, fg(YELLOW)))).collect();
    f.render_widget(Paragraph::new(mascot), Rect::new(head.x, head.y, brand::WIDTH, head.height));
    let title = Line::from(vec![
        Span::styled(s.settings.title.clone(), bold(YELLOW)),
        Span::styled(format!("  v{}", env!("CARGO_PKG_VERSION")), fg(GRAY)),
    ]);
    let text_x = head.x + brand::WIDTH + 2;
    f.render_widget(Paragraph::new(title), Rect::new(text_x, head.y + 1, head.width.saturating_sub(brand::WIDTH + 2), 1));

    // Abas: a escolhida em amarelo e sublinhada.
    let tab_line: Vec<Span> = TABS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == panel.tab { bold(YELLOW).add_modifier(Modifier::REVERSED) } else { fg(GRAY) };
            Span::styled(format!(" {name} "), style)
        })
        .collect();
    f.render_widget(Paragraph::new(Line::from(tab_line)), tabs);

    // Rótulos alinhados à esquerda; valores logo depois.
    let pairs = rows(panel.tab, s);
    let label_w = pairs.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0) + 2;
    let lines: Vec<Line> = pairs
        .into_iter()
        .map(|(label, value)| {
            Line::from(vec![
                Span::styled(format!("{label:<label_w$}"), fg(GRAY)),
                Span::raw(value),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), body);

    let help = "←/→ troca de aba · esc fecha";
    f.render_widget(Paragraph::new(Line::from(Span::styled(help, fg(GRAY)))), hint);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitclaw::skills;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::sync::{Arc, Mutex};

    // Catálogo vazio: o painel não consulta o provedor.
    struct NoCatalog;

    impl bitclaw::models::ModelCatalog for NoCatalog {
        fn list(&self) -> Result<Vec<bitclaw::models::ModelInfo>, String> {
            Ok(Vec::new())
        }
    }

    fn settings() -> Settings {
        Settings {
            title: "BitClaw".into(),
            info: vec![],
            skills: vec![],
            skills_off: skills::new_disabled(),
            model: Arc::new(Mutex::new(bitclaw::models::ModelInfo::unknown("m:1b"))),
            default_model: "m:1b".into(),
            catalog: Arc::new(NoCatalog),
            effort: Arc::new(Mutex::new(bitclaw::models::Effort::Off)),
            options: Arc::new(Mutex::new(bitclaw::ollama::Options::default())),
            default_options: bitclaw::ollama::Options::default(),
            auto_yes: false,
            color: false,
            mouse: false,
            root: std::path::PathBuf::from("/projeto"),
        }
    }

    fn snapshot<'a>(settings: &'a Settings, stats: &'a SessionStats, usage: Option<Usage>, model: &str) -> Snapshot<'a> {
        Snapshot { settings, model: model.into(), effort: "desligado".into(), usage, stats }
    }

    #[test]
    fn tabs_wrap_around_in_both_directions() {
        let mut p = StatusPanel::default();
        p.step(true);
        assert_eq!(p.tab, TABS.len() - 1, "voltar no começo cai na última");
        p.step(false);
        assert_eq!(p.tab, 0, "avançar na última volta ao começo");
    }

    #[test]
    fn the_status_tab_shows_the_model_and_says_when_it_was_switched() {
        let st = settings();
        let stats = SessionStats::new();
        let same = rows(0, &snapshot(&st, &stats, None, "m:1b"));
        assert!(same.iter().any(|(l, v)| *l == "Modelo" && v == "m:1b"));
        let switched = rows(0, &snapshot(&st, &stats, None, "outro:3b"));
        assert!(switched.iter().any(|(l, v)| *l == "Modelo" && v == "outro:3b · padrão m:1b"));
    }

    #[test]
    fn the_context_bar_fills_in_proportion() {
        assert_eq!(bar(0.0), "░".repeat(BAR_CELLS));
        assert_eq!(bar(50.0), format!("{}{}", "█".repeat(10), "░".repeat(10)));
        assert_eq!(bar(100.0), "█".repeat(BAR_CELLS));
    }

    #[test]
    fn stats_count_turns_and_steps() {
        let st = settings();
        let mut stats = SessionStats::new();
        stats.turns = 2;
        stats.steps = 5;
        let values: Vec<String> = rows(3, &snapshot(&st, &stats, None, "m:1b")).into_iter().map(|(_, v)| v).collect();
        assert_eq!(values[1], "2");
        assert_eq!(values[2], "5");
    }

    #[test]
    fn the_panel_draws_its_tabs_and_the_mascot() {
        let st = settings();
        let stats = SessionStats::new();
        let usage = Usage { tokens: 861, window: 16384, percent: 5.0 };
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        let panel = StatusPanel { tab: 2 };
        terminal.draw(|f| draw_status_panel(f, &snapshot(&st, &stats, Some(usage), "m:1b"), &panel)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let lines: Vec<String> = (0..buffer.area.height)
            .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string())
            .collect();
        assert!(lines[4].contains("Status") && lines[4].contains("Usage") && lines[4].contains("Stats"), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("861 de 16384")), "{lines:?}");
        assert!(lines[9].contains("esc fecha"), "{lines:?}");
    }
}

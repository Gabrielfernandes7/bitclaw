//! Entrada do usuário: teclas, painéis e comandos com `/`.

use super::*;
use ratatui_textarea::CursorMove;

impl App {
    // Teclas do painel de skills. Devolve `true` para sair (o painel nunca sai do programa).
    pub(super) fn picker_key(&mut self, key: KeyEvent) -> io::Result<bool> {
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

    // Teclas do painel `/status`: ←/→ trocam de aba, Esc, Enter ou Ctrl+C fecham.
    pub(super) fn status_panel_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(panel) = self.status_panel.as_mut() else { return Ok(false) };
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.status_panel = None,
            KeyCode::Char('c') if ctrl => self.status_panel = None,
            KeyCode::Right | KeyCode::Tab => panel.step(false),
            KeyCode::Left | KeyCode::BackTab => panel.step(true),
            _ => {}
        }
        Ok(false)
    }

    // Trata uma tecla. Devolve `true` para sair.
    pub(super) fn on_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Qualquer tecla encerra a saudação: o mascote para antes de a conversa começar.
        self.finish_greeting()?;

        // Com algo selecionado na conversa, Ctrl+C copia (como num terminal) em vez de interromper ou sair.
        // Qualquer outra tecla desfaz a seleção e segue normalmente.
        if self.transcript.selection.is_some() {
            if ctrl && key.code == KeyCode::Char('c') {
                self.copy_selection();
                self.transcript.selection = None;
                return Ok(false);
            }
            self.transcript.selection = None;
        }

        // Rolagem da conversa pelo teclado: vale sempre, até com painel aberto ou decisão pendente.
        if self.scroll_key(key) {
            return Ok(false);
        }

        // Decisão pendente: só as teclas de resposta valem.
        if let Some(work) = &mut self.work
            && let Some(reply) = work.approval.take()
        {
            match key.code {
                KeyCode::Char('s' | 'S' | 'y' | 'Y') if !ctrl => {
                    let _ = reply.send(true);
                }
                KeyCode::Char('a' | 'A') if !ctrl && work.approval_batchable => {
                    work.approve_all = true;
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
        if self.effort_panel.is_some() {
            return self.effort_panel_key(key);
        }
        if self.model_picker.is_some() {
            return self.model_picker_key(key);
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }
        if self.status_panel.is_some() {
            return self.status_panel_key(key);
        }

        // Sugestões de arquivo (`@`): ficam com ↑/↓, Tab, Enter e Esc enquanto aparecem.
        if let Some(m) = self.mention_matches() {
            let n = m.entries.len();
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
                    self.accept_mention(&m, chosen);
                    return Ok(false);
                }
                KeyCode::Enter if !key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
                    self.accept_mention(&m, chosen);
                    return Ok(false);
                }
                KeyCode::Esc => {
                    self.mention_off = true;
                    return Ok(false);
                }
                _ => {}
            }
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
                self.mention_off = false;
                self.history.edited();
            }
        }
        Ok(false)
    }

    // PageUp/PageDown rolam meia tela; Shift+↑/↓, uma linha; Ctrl+Home e Ctrl+End vão ao começo e ao fim.
    // Devolve `true` se a tecla era de rolagem.
    fn scroll_key(&mut self, key: KeyEvent) -> bool {
        let height = usize::from(self.chat_area.height);
        let page = (height / 2).max(1) as isize;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::PageUp => self.transcript.scroll(page, height),
            KeyCode::PageDown => self.transcript.scroll(-page, height),
            KeyCode::Up if shift => self.transcript.scroll(1, height),
            KeyCode::Down if shift => self.transcript.scroll(-1, height),
            KeyCode::Home if ctrl => self.transcript.scroll(isize::MAX / 2, height),
            KeyCode::End if ctrl => self.transcript.to_end(),
            _ => return false,
        }
        true
    }

    // A menção `@` sob o cursor e as sugestões para ela. `None` se não há menção, se `Esc` a fechou
    // ou se o agente está trabalhando.
    pub(super) fn mention_matches(&self) -> Option<Mention> {
        if self.work.is_some() || self.mention_off {
            return None;
        }
        let ratatui_textarea::DataCursor(row, col) = self.editor.cursor();
        let (start, token) = mention::token_at(self.editor.lines().get(row)?, col)?;
        let entries = mention::suggest(&self.settings.root, &token);
        (!entries.is_empty()).then_some(Mention { start, entries })
    }

    // Troca a menção pela sugestão escolhida e deixa o cursor depois dela.
    pub(super) fn accept_mention(&mut self, m: &Mention, chosen: usize) {
        let ratatui_textarea::DataCursor(row, col) = self.editor.cursor();
        let mut lines: Vec<String> = self.editor.lines().to_vec();
        let (line, new_col) = mention::apply(&lines[row], m.start, col, &m.entries[chosen]);
        lines[row] = line;
        self.editor = new_editor();
        self.editor.insert_str(lines.join("\n"));
        self.editor.move_cursor(CursorMove::Jump(row as u16, new_col as u16));
        self.menu_index = 0;
    }

    pub(super) fn interrupt(&mut self) {
        if let Some(work) = &mut self.work {
            work.cancelling = true;
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    // Envia o texto do editor: comando `/` ou tarefa para o agente. Devolve `true` para sair.
    pub(super) fn submit(&mut self, text: &str) -> io::Result<bool> {
        // Mandou algo: a conversa volta ao fim, para a resposta aparecer.
        self.transcript.to_end();
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
            _ if command.is_some_and(|c| c.name == "/restart-dev") => return self.restart_dev_command(),
            _ if command.is_some_and(|c| c.name == "/welcome-dev") => return self.leave_for(Next::Welcome),
            _ if command.is_some_and(|c| c.name == "/openrouter") => return self.leave_for(Next::OpenRouter),
            _ if command.is_some_and(|c| c.name == "/clear") => {
                let _ = self.commands.send(Command::Clear);
                self.clear_screen()?;
            }
            _ if command.is_some_and(|c| c.name == "/undo") => {
                let _ = self.commands.send(Command::Undo);
            }
            _ if command.is_some_and(|c| c.name == "/think") => self.think_command()?,
            _ if command.is_some_and(|c| c.name == "/model") => self.model_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/context") => self.context_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/temp") => self.temperature_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/effort") => self.effort_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/init") => self.init_command()?,
            _ if command.is_some_and(|c| c.name == "/plan") => self.plan_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/soul") => {
                let mut lines = match crate::app::soul::load(&self.settings.root) {
                    Some(text) => text.lines().map(|l| note(l)).collect(),
                    None => vec![note("sem SOUL.md. Rode `bitclaw soul` num terminal para responder às perguntas.")],
                };
                lines.push(note("mudanças no arquivo valem na próxima abertura do BitClaw."));
                lines.push(Line::default());
                self.print(lines)?;
            }
            _ if command.is_some_and(|c| c.name == "/skill") => self.skill_command(arg)?,
            _ if command.is_some_and(|c| c.name == "/skills") => {
                if self.settings.skills.is_empty() {
                    self.print(vec![note("nenhuma skill carregada."), Line::default()])?;
                } else {
                    self.picker = Some(Picker { query: String::new(), index: 0, by_name: false });
                }
            }
            _ if command.is_some_and(|c| c.name == "/status") => {
                self.status_panel = Some(StatusPanel::default());
            }
            _ if command.is_some_and(|c| c.name == "/help") => {
                let mut lines: Vec<Line<'static>> =
                    super::commands::visible().map(|c| note(&format!("{:<8} {}", c.name, c.desc))).collect();
                lines.push(note("enter envia · shift+enter ou ctrl+j nova linha · esc interrompe"));
                lines.push(note("roda do mouse, PageUp/PageDown ou shift+↑/↓ rolam a conversa · ctrl+end volta ao fim"));
                lines.push(note("arrastar seleciona e copia · ctrl+c com seleção copia de novo · --no-mouse devolve a seleção ao terminal"));
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
                let mut lines = markdown::plain(task, width, Span::styled("❯ ", bold(YELLOW)), Style::default(), &palette());
                lines.push(Line::default());
                self.print(lines)?;
                self.cancel.store(false, Ordering::Relaxed);
                self.stats.turns += 1;
                let _ = self.commands.send(Command::Submit(super::mention::expand(&self.settings.root, task)));
                self.work =
                    Some(Work::new("pensando"));
            }
        }
        Ok(false)
    }

    // `/clear`: apaga a tela e o scrollback e recomeça com o cabeçalho, como no Claude Code.
    pub(super) fn active_model(&self) -> ModelInfo {
        locked(&self.settings.model).clone()
    }

    pub(super) fn switch_model(&self, model: ModelInfo) {
        let _ = self.commands.send(Command::SwitchModel(model));
    }

    // `/think`: liga no padrão do modelo ou desliga. É um atalho de `/effort`.
    pub(super) fn think_command(&mut self) -> io::Result<()> {
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

    // `/plan [on|off]`: liga, desliga ou alterna o modo Plan. Vale já no próximo passo do agente.
    pub(super) fn plan_command(&mut self, arg: &str) -> io::Result<()> {
        use std::sync::atomic::Ordering::Relaxed;
        let now = self.settings.plan.load(Relaxed);
        let next = match arg {
            "" => !now,
            "on" | "ligar" | "sim" => true,
            "off" | "desligar" | "não" | "nao" => false,
            _ => return self.print(vec![note("uso: /plan [on|off]"), Line::default()]),
        };
        if self.work.is_some() {
            return self.print(vec![note("espere a tarefa terminar para mudar o modo Plan."), Line::default()]);
        }
        self.settings.plan.store(next, Relaxed);
        let _ = self.commands.send(Command::Plan(next));
        let text = if next {
            "modo Plan ligado: só leitura. Nada é editado, criado, removido nem executado. /plan desliga."
        } else {
            "modo Plan desligado: as ferramentas de alteração voltaram."
        };
        self.print(vec![note(text), Line::default()])
    }

    // `/init`: o agente monta o rascunho e pede aprovação; a prévia aparece como as outras edições.
    pub(super) fn init_command(&mut self) -> io::Result<()> {
        if self.work.is_some() {
            return self.print(vec![note("espere a tarefa terminar para criar o AGENTS.md."), Line::default()]);
        }
        self.cancel.store(false, Ordering::Relaxed);
        let _ = self.commands.send(Command::Init);
        self.work = Some(Work::new("montando AGENTS.md"));
        Ok(())
    }

    // `/skill install <origem>` baixa e mostra a prévia; `/skill yes` instala; `/skill no` descarta.
    pub(super) fn skill_command(&mut self, arg: &str) -> io::Result<()> {
        let (sub, rest) = arg.split_once(char::is_whitespace).map_or((arg, ""), |(s, r)| (s, r.trim()));
        let lines = match sub {
            "install" | "instalar" => match skill_install::parse_args(rest) {
                _ if self.skill_job.is_some() => vec![note("já estou baixando uma skill; espere terminar.")],
                Err(e) => vec![Line::from(Span::styled(format!("  {e}"), fg(YELLOW)))],
                Ok(spec) => {
                    self.skill_pending = None;
                    let (tx, rx) = mpsc::channel();
                    let shown = spec.url.clone();
                    thread::spawn(move || {
                        let _ = tx.send(skill_install::stage(&spec));
                    });
                    self.skill_job = Some(rx);
                    vec![note(&format!("baixando {shown}…"))]
                }
            },
            "yes" | "sim" | "y" => match self.skill_pending.take() {
                None => vec![note("nenhuma skill esperando aprovação. Use /skill install <origem>.")],
                Some(staged) => match skill_install::install(&staged, &self.settings.root) {
                    Ok(dest) => vec![
                        note(&format!("skill {} instalada em {}.", staged.skill.name, dest.display())),
                        note("ela aparece em /skills quando o BitClaw for aberto de novo."),
                    ],
                    Err(e) => vec![Line::from(Span::styled(format!("  não instalei: {e}"), fg(RED)))],
                },
            },
            "no" | "não" | "nao" | "n" => match self.skill_pending.take() {
                None => vec![note("nenhuma skill esperando aprovação.")],
                Some(staged) => vec![note(&format!("skill {} descartada; nada foi instalado.", staged.skill.name))],
            },
            _ => vec![note(skill_install::USAGE), note("depois: /skill yes para instalar ou /skill no para descartar.")],
        };
        let mut lines = lines;
        lines.push(Line::default());
        self.print(lines)
    }

    // Quando o download termina, mostra a prévia e guarda a skill até o usuário responder.
    pub(super) fn poll_skill_job(&mut self) -> io::Result<()> {
        let Some(rx) = &self.skill_job else { return Ok(()) };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return Ok(()),
            Err(TryRecvError::Disconnected) => Err("o download terminou sem resposta".to_string()),
        };
        self.skill_job = None;
        let mut lines: Vec<Line<'static>> = match result {
            Ok(staged) => {
                let mut lines: Vec<Line<'static>> = skill_install::preview(&staged).iter().map(|l| note(l)).collect();
                lines.push(Line::from(Span::styled("  /skill yes instala · /skill no descarta", bold(YELLOW))));
                self.skill_pending = Some(staged);
                lines
            }
            Err(e) => vec![Line::from(Span::styled(format!("  {e}"), fg(RED)))],
        };
        lines.push(Line::default());
        self.print(lines)
    }

    // Sai da interface para `next`; o `run` faz a ação depois. Não sai com uma tarefa em andamento.
    pub(super) fn leave_for(&mut self, next: Next) -> io::Result<bool> {
        if self.work.is_some() {
            self.print(vec![note("espere a tarefa terminar para sair."), Line::default()])?;
            return Ok(false);
        }
        self.next = next;
        Ok(true)
    }

    // `/restart-dev`: recompila e, se deu certo, sai para o `run` reabrir o binário novo. Se a compilação
    // falhar, mostra o fim da saída do compilador e continua na sessão atual. Devolve `true` para sair.
    pub(super) fn restart_dev_command(&mut self) -> io::Result<bool> {
        if self.work.is_some() {
            self.print(vec![note("espere a tarefa terminar para reiniciar."), Line::default()])?;
            return Ok(false);
        }
        self.print(vec![note("recompilando…")])?;
        match super::restart::build() {
            Ok(()) => {
                self.next = Next::Restart;
                Ok(true)
            }
            Err(msg) => {
                let mut lines: Vec<Line<'static>> = vec![Line::from(Span::styled("  a compilação falhou; continuo na versão atual:", fg(RED)))];
                lines.extend(msg.lines().map(|l| note(l)));
                lines.push(Line::default());
                self.print(lines)?;
                Ok(false)
            }
        }
    }

    // `/effort`: sem argumento abre o painel deslizante com os níveis deste modelo; com um, valida e aplica.
    pub(super) fn effort_command(&mut self, arg: &str) -> io::Result<()> {
        let model = self.active_model();
        if arg.is_empty() {
            let current = *locked(&self.settings.effort);
            self.effort_panel = Some(EffortPanel::new(model.capabilities.supported_efforts(), current));
            return Ok(());
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

    // Teclas do painel de esforço. ←/→ mudam o nível, Enter aplica (pelo mesmo caminho do `/effort <nível>`) e Esc fecha.
    pub(super) fn effort_panel_key(&mut self, key: KeyEvent) -> io::Result<bool> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(p) = self.effort_panel.as_mut() else { return Ok(false) };
        let chosen = p.chosen();
        match key.code {
            KeyCode::Esc => self.effort_panel = None,
            KeyCode::Char('c') if ctrl => self.effort_panel = None,
            KeyCode::Left => p.step(false),
            KeyCode::Right => p.step(true),
            KeyCode::Enter => {
                self.effort_panel = None;
                if let Some(effort) = chosen {
                    self.effort_command(effort.as_str())?;
                }
            }
            _ => {}
        }
        Ok(false)
    }

    // `/context`: sem argumento mostra a janela e o que muda ao mexer nela; com um valor (ou `reset`), troca.
    pub(super) fn context_command(&mut self, arg: &str) -> io::Result<()> {
        let default = self.settings.default_options.num_ctx as usize;
        let current = locked(&self.settings.options).num_ctx as usize;
        let max = self.active_model().capabilities.context_window;
        let target = match parse_context(arg) {
            Err(e) => return self.print(vec![note(&e), Line::default()]),
            Ok(Setting::Show) => {
                let limit = max.map_or(String::new(), |m| format!(" · máximo do modelo {m}"));
                let mut lines = vec![
                    note(&format!("contexto {current} tokens · padrão {default}{limit}")),
                    note("troque com /context <tokens> (16384 ou 16k) e volte ao padrão com /context reset."),
                    note(brand::CONTEXT_RAISE_HEAD),
                ];
                lines.extend(brand::CONTEXT_COSTS.iter().map(|t| note(&format!("• {t}"))));
                lines.push(Line::default());
                return self.print(lines);
            }
            Ok(Setting::Reset) => default,
            Ok(Setting::Set(n)) => n,
        };
        if self.work.is_some() {
            return self.print(vec![note("espere a tarefa terminar para mudar o contexto."), Line::default()]);
        }
        if let Some(m) = max.filter(|&m| target > m) {
            return self.print(vec![note(&format!("este modelo aceita no máximo {m} tokens.")), Line::default()]);
        }
        if target == current {
            return self.print(vec![note(&format!("o contexto já está em {current} tokens.")), Line::default()]);
        }
        let _ = self.commands.send(Command::SetContext(target));
        let mut lines = if target > current {
            let mut lines = self.warning(brand::CONTEXT_RAISE_HEAD)?;
            lines.extend(brand::CONTEXT_COSTS.iter().map(|t| note(&format!("• {t}"))));
            lines
        } else {
            self.warning(brand::CONTEXT_LOWER)?
        };
        lines.push(Line::default());
        self.print(lines)
    }

    // `/temp`: sem argumento mostra a temperatura e o que ela muda; com um valor (ou `reset`), troca.
    pub(super) fn temperature_command(&mut self, arg: &str) -> io::Result<()> {
        let default = self.settings.default_options.temperature;
        let current = locked(&self.settings.options).temperature;
        let target = match parse_temperature(arg) {
            Err(e) => return self.print(vec![note(&e), Line::default()]),
            Ok(Setting::Show) => {
                let mut lines = vec![
                    note(&format!("temperatura {current} · padrão {default}")),
                    note("troque com /temp <0 a 2> e volte ao padrão com /temp reset."),
                ];
                lines.extend(brand::TEMPERATURE_NOTES.iter().map(|t| note(&format!("• {t}"))));
                lines.push(Line::default());
                return self.print(lines);
            }
            Ok(Setting::Reset) => default,
            Ok(Setting::Set(t)) => t,
        };
        locked(&self.settings.options).temperature = target;
        self.print(vec![note(&format!("temperatura {target} · {}", temperature_effect(target))), Line::default()])
    }

    // `/model`: sem argumento abre o painel; com um id, troca direto. A troca espera a tarefa atual.
    pub(super) fn model_command(&mut self, arg: &str) -> io::Result<()> {
        let action = parse_model_arg(arg);
        if let ModelAction::Show(name) = &action {
            return self.model_show(name.as_deref());
        }
        if self.work.is_some() {
            return self.print(vec![note("espere a tarefa terminar para trocar de modelo."), Line::default()]);
        }
        if let ModelAction::Load(name) = &action {
            return match self.settings.catalog.find(name) {
                Ok(Some(model)) => {
                    self.switch_model(model);
                    Ok(())
                }
                Ok(None) => self.print(vec![note(&format!("modelo não instalado: {name}. Veja /model.")), Line::default()]),
                Err(e) => self.print(vec![note(&format!("não consegui consultar os modelos: {e}")), Line::default()]),
            };
        }
        match self.settings.catalog.list() {
            Ok(models) if models.is_empty() => {
                self.print(vec![note("nenhum modelo disponível neste provedor (no Ollama, instale com: ollama pull <modelo>)"), Line::default()])
            }
            Ok(models) => {
                self.model_picker = Some(ModelPicker { query: String::new(), index: 0, models });
                Ok(())
            }
            Err(e) => self.print(vec![note(&format!("não consegui listar os modelos: {e}")), Line::default()]),
        }
    }

    // `/model show [nome]`: ficha do modelo pedido, ou do ativo se não vier nome.
    fn model_show(&mut self, name: Option<&str>) -> io::Result<()> {
        let active = self.active_model();
        let model = match name {
            None => active.clone(),
            Some(n) => match self.settings.catalog.find(n) {
                Ok(Some(m)) => m,
                Ok(None) => return self.print(vec![note(&format!("modelo não instalado: {n}. Veja /model.")), Line::default()]),
                Err(e) => return self.print(vec![note(&format!("não consegui consultar os modelos: {e}")), Line::default()]),
            },
        };
        let mut lines = model_card(&model, model.id == active.id);
        lines.push(Line::default());
        self.print(lines)
    }

    // Teclas do painel de modelos. Enter escolhe e fecha; Esc fecha sem trocar. Devolve `true` para sair (nunca sai).
    pub(super) fn model_picker_key(&mut self, key: KeyEvent) -> io::Result<bool> {
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
}

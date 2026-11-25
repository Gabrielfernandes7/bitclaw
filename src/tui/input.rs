//! Entrada do usuário: teclas, painéis e comandos com `/`.

use super::*;

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

    // Trata uma tecla. Devolve `true` para sair.
    pub(super) fn on_key(&mut self, key: KeyEvent) -> io::Result<bool> {
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

    pub(super) fn interrupt(&mut self) {
        if let Some(work) = &mut self.work {
            work.cancelling = true;
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    // Envia o texto do editor: comando `/` ou tarefa para o agente. Devolve `true` para sair.
    pub(super) fn submit(&mut self, text: &str) -> io::Result<bool> {
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

    // `/effort`: sem argumento mostra o nível atual e os níveis deste modelo; com um, valida e aplica.
    pub(super) fn effort_command(&mut self, arg: &str) -> io::Result<()> {
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
                self.print(vec![note("nenhum modelo instalado. Instale com: ollama pull <modelo>"), Line::default()])
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

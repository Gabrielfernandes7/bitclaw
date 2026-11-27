//! `bitclaw openrouter`: configura o OpenRouter em três passos (chave, modelo, arquivo do agente) e deixa
//! `bitclaw --openrouter` pronto para usar. `bitclaw openrouter status` confere o que está em vigor e
//! `bitclaw openrouter logout` desfaz.
//!
//! A chave é lida sem aparecer na tela, conferida no OpenRouter e, se a pessoa quiser, guardada em
//! `~/.config/bitclaw/openrouter.key` com permissão só do dono. Ela nunca vai para o TOML (que é versionado):
//! o agente gerado, `~/.config/bitclaw/openrouter.toml`, só leva o nome da variável. A variável
//! `OPENROUTER_API_KEY` sempre vale mais que o arquivo.

use bitclaw::adapters::openrouter::{Account, ApiKey, DEFAULT_BASE_URL, DEFAULT_KEY_ENV, OpenRouterCatalog, check_key};
use bitclaw::models::{ModelCatalog, ModelInfo};
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

// O agente de partida: o mesmo de `agents/openrouter.toml`, com o modelo e a janela trocados.
const TEMPLATE: &str = include_str!("../../agents/openrouter.toml");
// Quantos modelos mostrar por vez.
const SHOWN: usize = 12;
// Janela usada quando o OpenRouter não informa, e teto para a janela de um modelo muito grande.
const DEFAULT_WINDOW: usize = 32_768;
const MAX_WINDOW: usize = 131_072;

pub fn config_path() -> Option<PathBuf> {
    super::update::config_dir().map(|d| d.join("openrouter.toml"))
}

pub fn key_path() -> Option<PathBuf> {
    super::update::config_dir().map(|d| d.join("openrouter.key"))
}

// A chave em vigor: a variável `var` ou, sem ela, a guardada por `bitclaw openrouter`.
pub fn api_key(var: &str) -> Result<ApiKey, String> {
    ApiKey::from_env_or_file(var, key_path().as_deref())
}


pub trait Net {
    fn account(&self, key: &ApiKey) -> Result<Account, String>;
    fn models(&self, key: ApiKey) -> Result<Vec<ModelInfo>, String>;
}

pub struct Live;

impl Net for Live {
    fn account(&self, key: &ApiKey) -> Result<Account, String> {
        check_key(DEFAULT_BASE_URL, key)
    }

    fn models(&self, key: ApiKey) -> Result<Vec<ModelInfo>, String> {
        OpenRouterCatalog::new(DEFAULT_BASE_URL.into(), Some(key)).list()
    }
}


// Gratuitos (`:free`) primeiro, o `default` na frente de todos, e só os que casam com `query`.
fn rank_models(mut models: Vec<ModelInfo>, default: &str, query: &str) -> Vec<ModelInfo> {
    let q = query.trim().to_lowercase();
    models.retain(|m| q.is_empty() || m.id.to_lowercase().contains(&q));
    models.sort_by_key(|m| (m.id != default, !m.id.ends_with(":free")));
    models
}

fn window_label(m: &ModelInfo) -> String {
    m.capabilities.context_window.map_or("janela ?".into(), |w| format!("janela {}k", w / 1000))
}

// O agente do OpenRouter com o modelo e a janela escolhidos. Só muda `name` e `context_window` de `[model]`.
pub fn render_config(template: &str, model: &str, window: Option<usize>) -> String {
    let window = window.map_or(DEFAULT_WINDOW, |w| w.clamp(8192, MAX_WINDOW));
    let mut out = String::from("# Gerado por `bitclaw openrouter`. Rode com: bitclaw --openrouter\n# A chave NÃO fica aqui: vem da variável abaixo ou de ~/.config/bitclaw/openrouter.key.\n");
    let mut in_model = false;
    let mut header = true;
    for line in template.lines() {
        // Os comentários do alto do molde falam de `--config`; os novos já os substituem.
        if header && (line.starts_with('#') || line.trim().is_empty()) {
            continue;
        }
        header = false;
        if line.starts_with('[') {
            in_model = line.trim() == "[model]";
        }
        if in_model && line.trim_start().starts_with("name =") {
            out.push_str(&format!("name = \"{model}\"\n"));
        } else if in_model && line.trim_start().starts_with("context_window =") {
            out.push_str(&format!("context_window = {window}\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn save_key(path: &Path, key: &ApiKey) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let _ = fs::remove_file(path);
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    writeln!(f, "{}", key.expose_for_saving())
}

fn describe(a: &Account) -> String {
    let mut parts = Vec::new();
    if let Some(label) = &a.label {
        parts.push(format!("chave \"{label}\""));
    }
    parts.push(match a.remaining() {
        Some(r) => format!("crédito restante US$ {r:.2}"),
        None => format!("sem teto de crédito (gasto US$ {:.2})", a.usage),
    });
    if a.free_tier {
        parts.push("conta gratuita: só modelos :free".into());
    }
    parts.join(", ")
}


pub struct Ctx {
    pub dir: PathBuf,
    // Nome da variável de ambiente da chave.
    pub var: String,
}

impl Ctx {
    fn key_file(&self) -> PathBuf {
        self.dir.join("openrouter.key")
    }
    fn config_file(&self) -> PathBuf {
        self.dir.join("openrouter.toml")
    }
}

fn ask(input: &mut dyn BufRead, out: &mut dyn Write, text: &str) -> Result<String, String> {
    write!(out, "{text}").and_then(|_| out.flush()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) => Err("entrada encerrada: nada foi alterado.".into()),
        Ok(_) => Ok(line.trim().to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn yes(answer: &str, default: bool) -> bool {
    match answer.to_lowercase().as_str() {
        "" => default,
        "s" | "sim" | "y" | "yes" => true,
        _ => false,
    }
}

// Passo 1: a chave. Devolve a chave conferida e se ela veio digitada agora (e pode ser guardada).
fn step_key(
    ctx: &Ctx,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    secret: &mut dyn FnMut(&mut dyn BufRead, &mut dyn Write) -> io::Result<String>,
    net: &dyn Net,
) -> Result<(ApiKey, bool), String> {
    let w = |e: io::Error| e.to_string();
    writeln!(out, "1/3 Chave de API").map_err(w)?;
    let in_env = std::env::var(&ctx.var).is_ok_and(|v| !v.trim().is_empty());
    if let Ok(key) = ApiKey::from_env_or_file(&ctx.var, Some(&ctx.key_file())) {
        let from = if in_env { format!("da variável {}", ctx.var) } else { "guardada em ~/.config/bitclaw/openrouter.key".into() };
        match net.account(&key) {
            Ok(a) => {
                writeln!(out, "    usando a chave {from}: {}", describe(&a)).map_err(w)?;
                return Ok((key, false));
            }
            Err(e) => writeln!(out, "    a chave {from} não passou: {e}").map_err(w)?,
        }
    }
    writeln!(out, "    Crie uma em https://openrouter.ai/keys (há modelos gratuitos, sem cartão).").map_err(w)?;
    for attempt in 1..=3 {
        write!(out, "    Cole a chave (não aparece na tela): ").and_then(|_| out.flush()).map_err(w)?;
        let typed = match secret(input, out) {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(out);
                return Err(if e.kind() == io::ErrorKind::Interrupted { "cancelado: nada foi alterado.".into() } else { e.to_string() });
            }
        };
        writeln!(out).map_err(w)?;
        let Some(key) = ApiKey::from_value(&typed) else {
            writeln!(out, "    nada digitado.").map_err(w)?;
            continue;
        };
        match net.account(&key) {
            Ok(a) => {
                writeln!(out, "    ok: {}", describe(&a)).map_err(w)?;
                return Ok((key, true));
            }
            Err(e) if e.contains("recusou") && attempt < 3 => writeln!(out, "    {e}").map_err(w)?,
            Err(e) => return Err(e),
        }
    }
    Err("não consegui uma chave válida: nada foi alterado.".into())
}

// Passo 2: o modelo. Enter fica com o sugerido; um número escolhe da lista; texto filtra a lista.
fn step_model(
    key: ApiKey,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    net: &dyn Net,
) -> Result<ModelInfo, String> {
    let w = |e: io::Error| e.to_string();
    writeln!(out, "2/3 Modelo").map_err(w)?;
    let all = net.models(key)?;
    if all.is_empty() {
        return Err("o OpenRouter não listou nenhum modelo que chame ferramentas.".into());
    }
    let default_id = "baidu/ernie-5.5-flash-preview:free";
    let suggested = rank_models(all.clone(), default_id, "").remove(0);
    writeln!(out, "    {} modelos chamam ferramentas (gratuitos primeiro).", all.len()).map_err(w)?;
    let mut query = String::new();
    for _ in 0..8 {
        let list = rank_models(all.clone(), default_id, &query);
        if list.is_empty() {
            writeln!(out, "    nenhum modelo casa com \"{query}\".").map_err(w)?;
        }
        for (i, m) in list.iter().take(SHOWN).enumerate() {
            let free = if m.id.ends_with(":free") { "gratuito" } else { "pago" };
            writeln!(out, "    {:>2}  {}  ({free}, {})", i + 1, m.id, window_label(m)).map_err(w)?;
        }
        if list.len() > SHOWN {
            writeln!(out, "    … e mais {}: digite parte do nome para filtrar.", list.len() - SHOWN).map_err(w)?;
        }
        let answer = ask(input, out, &format!("    Número, id completo ou texto para filtrar [Enter = {}]: ", suggested.id))?;
        if answer.is_empty() {
            return Ok(suggested);
        }
        if let Some(m) = answer.parse::<usize>().ok().and_then(|n| list.get(n.wrapping_sub(1))) {
            return Ok(m.clone());
        }
        if let Some(m) = all.iter().find(|m| m.id == answer) {
            return Ok(m.clone());
        }
        query = answer;
    }
    Err("não escolhi um modelo: nada foi alterado.".into())
}

// O comando inteiro, sem terminal e sem rede por baixo: tudo entra por parâmetro.
pub fn run_with(
    ctx: &Ctx,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    secret: &mut dyn FnMut(&mut dyn BufRead, &mut dyn Write) -> io::Result<String>,
    net: &dyn Net,
) -> Result<String, String> {
    let w = |e: io::Error| e.to_string();
    writeln!(out, "OpenRouter em três passos. Nada é gravado antes do fim.").map_err(w)?;
    let (key, typed) = step_key(ctx, input, out, secret, net)?;
    let save = typed && yes(&ask(input, out, "    Guardar a chave em ~/.config/bitclaw/openrouter.key (só você lê)? [S/n] ")?, true);
    let model = step_model(ApiKey::from_value(key.expose_for_saving()).ok_or("chave vazia")?, input, out, net)?;

    writeln!(out, "3/3 Gravando").map_err(w)?;
    fs::create_dir_all(&ctx.dir).map_err(|e| format!("não consegui criar {}: {e}", ctx.dir.display()))?;
    if save {
        save_key(&ctx.key_file(), &key).map_err(|e| format!("não consegui guardar a chave: {e}"))?;
        writeln!(out, "    chave guardada em {} (permissão 600)", ctx.key_file().display()).map_err(w)?;
    }
    let text = render_config(TEMPLATE, &model.id, model.capabilities.context_window);
    fs::write(ctx.config_file(), text).map_err(|e| format!("não consegui gravar {}: {e}", ctx.config_file().display()))?;
    writeln!(out, "    agente gravado em {} (modelo {})", ctx.config_file().display(), model.id).map_err(w)?;

    let how_key = if save || !typed { String::new() } else { format!("\nA chave não foi guardada: exporte `export {}=...` antes de abrir.", ctx.var) };
    Ok(format!("Pronto. Abra com: bitclaw --openrouter{how_key}\nTrocar de modelo depois: /model dentro do BitClaw, ou rode `bitclaw openrouter` de novo."))
}

// Lê a chave sem ecoar: modo raw do terminal, um `*` por caractere. Sem terminal (pipe), lê uma linha de `input`
// (o mesmo leitor do resto do fluxo: a trava do stdin não pode ser pedida duas vezes).
pub fn read_secret(input: &mut dyn BufRead, out: &mut dyn Write) -> io::Result<String> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use io::IsTerminal;
    if !io::stdin().is_terminal() {
        let mut line = String::new();
        input.read_line(&mut line)?;
        return Ok(line.trim().to_string());
    }
    struct Raw;
    impl Drop for Raw {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    crossterm::terminal::enable_raw_mode()?;
    let _raw = Raw;
    let mut text = String::new();
    loop {
        let Event::Key(k) = event::read()? else { continue };
        if k.kind == KeyEventKind::Release {
            continue;
        }
        match k.code {
            KeyCode::Enter => return Ok(text),
            KeyCode::Esc => return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelado")),
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelado"));
            }
            KeyCode::Backspace => {
                if text.pop().is_some() {
                    write!(out, "\x08 \x08")?;
                    out.flush()?;
                }
            }
            KeyCode::Char(c) => {
                text.push(c);
                write!(out, "*")?;
                out.flush()?;
            }
            _ => {}
        }
    }
}

// `bitclaw openrouter`
pub fn run() -> Result<String, String> {
    let dir = super::update::config_dir().ok_or("sem HOME: não sei onde gravar")?;
    let ctx = Ctx { dir, var: DEFAULT_KEY_ENV.into() };
    run_with(&ctx, &mut io::stdin().lock(), &mut io::stdout(), &mut read_secret, &Live)
}

// `bitclaw openrouter status`
pub fn status() -> Result<String, String> {
    let var = DEFAULT_KEY_ENV;
    let mut lines = Vec::new();
    let in_env = std::env::var(var).is_ok_and(|v| !v.trim().is_empty());
    match api_key(var) {
        Ok(key) => {
            let from = if in_env { format!("variável {var}") } else { "arquivo ~/.config/bitclaw/openrouter.key".to_string() };
            lines.push(match Live.account(&key) {
                Ok(a) => format!("chave: ok ({from}): {}", describe(&a)),
                Err(e) => format!("chave: {from} NÃO passou: {e}"),
            });
        }
        Err(e) => lines.push(format!("chave: {e}")),
    }
    lines.push(match config_path().filter(|p| p.is_file()) {
        Some(p) => {
            let model = fs::read_to_string(&p)
                .ok()
                .and_then(|t| t.parse::<toml::Table>().ok())
                .and_then(|t| t.get("model")?.get("name")?.as_str().map(String::from))
                .unwrap_or_else(|| "?".into());
            format!("agente: {} (modelo {model}). Abra com: bitclaw --openrouter", p.display())
        }
        None => "agente: ainda não há. Rode `bitclaw openrouter`.".into(),
    });
    Ok(lines.join("\n"))
}

// `bitclaw openrouter logout`: apaga a chave guardada e o agente gerado. A variável de ambiente não é tocada.
pub fn logout() -> Result<String, String> {
    let mut gone = Vec::new();
    for p in [key_path(), config_path()].into_iter().flatten() {
        if p.is_file() {
            fs::remove_file(&p).map_err(|e| format!("não consegui apagar {}: {e}", p.display()))?;
            gone.push(p.display().to_string());
        }
    }
    Ok(if gone.is_empty() {
        "não havia chave guardada nem agente do OpenRouter.".into()
    } else {
        format!("apagado: {}. A variável {DEFAULT_KEY_ENV}, se existir, não foi tocada.", gone.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitclaw::models::{ModelCapabilities, Reasoning};
    use std::io::Cursor;

    fn model(id: &str, window: Option<usize>) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            provider: "openrouter".into(),
            capabilities: ModelCapabilities { tool_calling: true, reasoning: Reasoning::Unsupported, context_window: window },
        }
    }

    struct Fake {
        valid: &'static str,
        models: Vec<ModelInfo>,
    }

    impl Net for Fake {
        fn account(&self, key: &ApiKey) -> Result<Account, String> {
            if key.expose_for_saving() == self.valid {
                Ok(Account { label: Some("teste".into()), usage: 1.0, limit: Some(4.0), free_tier: false })
            } else {
                Err("o OpenRouter recusou a chave (HTTP 401): ela está certa e ativa?".into())
            }
        }
        fn models(&self, _: ApiKey) -> Result<Vec<ModelInfo>, String> {
            Ok(self.models.clone())
        }
    }

    fn fake() -> Fake {
        Fake {
            valid: "sk-or-boa",
            models: vec![
                model("zeta/pago", Some(200_000)),
                model("acme/rapido:free", Some(32_000)),
                model("baidu/ernie-5.5-flash-preview:free", Some(32_768)),
                model("acme/grande", Some(1_000_000)),
            ],
        }
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bitclaw-or-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn go(dir: &Path, typed_key: &str, answers: &str) -> (Result<String, String>, String) {
        let ctx = Ctx { dir: dir.to_path_buf(), var: "BITCLAW_TESTE_OR_SEM_VARIAVEL".into() };
        let mut out = Vec::new();
        let key = typed_key.to_string();
        let r = run_with(&ctx, &mut Cursor::new(answers.to_string()), &mut out, &mut |_, _| Ok(key.clone()), &fake());
        (r, String::from_utf8(out).unwrap())
    }

    #[test]
    fn free_models_come_first_with_the_default_on_top_and_the_query_filters() {
        let ranked = rank_models(fake().models, "baidu/ernie-5.5-flash-preview:free", "");
        let ids: Vec<&str> = ranked.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids[0], "baidu/ernie-5.5-flash-preview:free");
        assert_eq!(ids[1], "acme/rapido:free");
        assert_eq!(rank_models(fake().models, "x", "ACME").len(), 2);
        assert!(rank_models(fake().models, "x", "nada").is_empty());
    }

    #[test]
    fn the_config_changes_only_the_model_name_and_the_window() {
        let text = render_config(TEMPLATE, "acme/grande", Some(1_000_000));
        assert!(text.starts_with("# Gerado por `bitclaw openrouter`"));
        assert!(text.contains("name = \"acme/grande\"") && !text.contains("baidu/ernie"));
        assert!(text.contains("context_window = 131072"), "janela limitada ao teto");
        assert!(text.contains("name = \"BitClaw\""), "o nome do agente não muda");
        assert!(text.contains("api_key_env = \"OPENROUTER_API_KEY\"") && !text.contains("sk-"));
        // O resultado ainda é um agente válido.
        bitclaw::config::AgentConfig::from_toml(&text).expect("configuração válida");
        assert!(render_config(TEMPLATE, "m/x", None).contains("context_window = 32768"));
    }

    #[test]
    fn the_full_flow_saves_the_key_privately_and_writes_a_valid_agent() {
        let dir = temp("flow");
        // chave boa; guardar (Enter = S); escolher o 2º da lista.
        let (r, out) = go(&dir, "sk-or-boa", "\n2\n");
        let msg = r.unwrap();
        assert!(msg.contains("bitclaw --openrouter"), "{msg}");
        assert!(out.contains("crédito restante US$ 3.00"), "{out}");
        assert!(!out.contains("sk-or-boa"), "a chave nunca aparece na saída: {out}");
        let saved = fs::read_to_string(dir.join("openrouter.key")).unwrap();
        assert_eq!(saved.trim(), "sk-or-boa");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(dir.join("openrouter.key").metadata().unwrap().permissions().mode() & 0o777, 0o600);
        }
        let cfg = fs::read_to_string(dir.join("openrouter.toml")).unwrap();
        assert!(cfg.contains("name = \"acme/rapido:free\""), "{cfg}");
        assert!(!cfg.contains("sk-or-boa"));
    }

    #[test]
    fn declining_to_save_keeps_the_key_off_the_disk_and_says_how_to_export_it() {
        let dir = temp("nosave");
        let (r, _) = go(&dir, "sk-or-boa", "n\n\n");
        let msg = r.unwrap();
        assert!(!dir.join("openrouter.key").exists());
        assert!(msg.contains("export OPENROUTER_API_KEY") || msg.contains("export BITCLAW_TESTE_OR_SEM_VARIAVEL"), "{msg}");
        assert!(dir.join("openrouter.toml").exists());
    }

    #[test]
    fn a_wrong_key_stops_after_three_tries_and_writes_nothing() {
        let dir = temp("bad");
        let (r, out) = go(&dir, "sk-errada", "");
        assert!(r.is_err(), "{r:?}");
        assert!(!out.contains("sk-errada"));
        assert!(!dir.join("openrouter.key").exists() && !dir.join("openrouter.toml").exists());
    }

    #[test]
    fn typing_text_filters_the_list_and_a_full_id_picks_that_model() {
        let dir = temp("filter");
        let (r, out) = go(&dir, "sk-or-boa", "s\nzeta\n1\n");
        r.unwrap();
        assert!(out.contains("zeta/pago") && out.contains("pago"), "{out}");
        let cfg = fs::read_to_string(dir.join("openrouter.toml")).unwrap();
        assert!(cfg.contains("name = \"zeta/pago\"") && cfg.contains("context_window = 131072"), "{cfg}");
        let dir2 = temp("fullid");
        go(&dir2, "sk-or-boa", "s\nacme/grande\n").0.unwrap();
        assert!(fs::read_to_string(dir2.join("openrouter.toml")).unwrap().contains("acme/grande"));
    }

    #[test]
    fn closing_the_input_mid_way_changes_nothing() {
        let dir = temp("eof");
        let (r, _) = go(&dir, "sk-or-boa", "s\n");
        assert!(r.is_err());
        assert!(!dir.join("openrouter.toml").exists() && !dir.join("openrouter.key").exists());
    }
}

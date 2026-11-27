//! `bitclaw update`: troca o binário instalado pela versão do último release do GitHub.
//!
//! O contrato com o release (`.github/workflows/release.yml` e `install.sh` seguem o mesmo):
//! - tag `vX.Y.Z`;
//! - um arquivo por plataforma, `bitclaw-<target>.tar.gz`, com o binário `bitclaw` dentro;
//! - `SHA256SUMS`, no formato do `shasum -a 256` (`<hex>  <arquivo>`).
//!
//! A rede fica em `latest_tag` e `download`. O resto (versão, soma, extração e troca do binário) é
//! local e tem teste sem rede.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
// `https://github.com/dono/repo`, do `Cargo.toml`.
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

// Endereços do GitHub. As variáveis existem para testar um release falso, não para uso comum.
fn api_base() -> String {
    std::env::var("BITCLAW_API_BASE").unwrap_or_else(|_| "https://api.github.com".into())
}

fn download_base() -> String {
    std::env::var("BITCLAW_DOWNLOAD_BASE").unwrap_or_else(|_| format!("{REPOSITORY}/releases/download"))
}

// `dono/repo` a partir do endereço do repositório.
fn slug() -> &'static str {
    REPOSITORY.trim_end_matches('/').trim_start_matches("https://github.com/")
}

// O alvo do binário em execução, com o nome que o release usa. `None` em plataforma sem release.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

pub fn asset_name(target: &str) -> String {
    format!("bitclaw-{target}.tar.gz")
}

// `v1.2.3` ou `1.2.3` em números. Sufixos (`-rc.1`) e partes não numéricas fazem a versão ser ignorada.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split('.');
    let n = |p: Option<&str>| p.and_then(|s| s.parse::<u64>().ok());
    let version = (n(parts.next())?, n(parts.next())?, n(parts.next())?);
    parts.next().is_none().then_some(version)
}

// Se `latest` é mais nova que `current`. Versões que não se leem nunca contam como mais novas.
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

// A soma de `asset` no conteúdo de um `SHA256SUMS`.
fn expected_sum<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hex, name) = line.trim().split_once(char::is_whitespace)?;
        (name.trim().trim_start_matches('*') == asset).then_some(hex)
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn http() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("bitclaw/{VERSION}"))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

// Tag do último release publicado (sem rascunhos nem pré-releases, pela própria API do GitHub).
pub fn latest_tag(timeout: Duration) -> Result<String, String> {
    let url = format!("{}/repos/{}/releases/latest", api_base(), slug());
    let resp = http()?.get(&url).timeout(timeout).send().map_err(|e| format!("não consegui consultar {url}: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("nenhum release encontrado em {REPOSITORY}: ainda não há release publicado, ou o repositório é privado"));
    }
    if !resp.status().is_success() {
        return Err(format!("o GitHub respondeu {} para {url}", resp.status()));
    }
    let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    body["tag_name"].as_str().map(str::to_string).ok_or_else(|| "resposta do GitHub sem tag_name".into())
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let resp = http()?.get(url).send().map_err(|e| format!("não consegui baixar {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url}: {}", resp.status()));
    }
    resp.bytes().map(|b| b.to_vec()).map_err(|e| e.to_string())
}

// Confere a soma, extrai o binário e o põe no lugar de `exe`. A troca é um `rename` na mesma pasta:
// atômica no Unix, e o processo em execução continua com o binário antigo até sair.
pub fn install(archive: &[u8], sums: &str, asset: &str, exe: &Path, work: &Path) -> Result<(), String> {
    let expected = expected_sum(sums, asset).ok_or_else(|| format!("SHA256SUMS não tem {asset}"))?;
    let actual = sha256_hex(archive);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!("soma de {asset} não confere: esperada {expected}, obtida {actual}. Nada foi trocado."));
    }
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let tarball = work.join(asset);
    fs::write(&tarball, archive).map_err(|e| e.to_string())?;
    let out = Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(work)
        .output()
        .map_err(|e| format!("não consegui rodar o tar: {e}"))?;
    if !out.status.success() {
        return Err(format!("tar falhou: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let fresh = work.join("bitclaw");
    if !fresh.is_file() {
        return Err(format!("{asset} não tem o binário bitclaw"));
    }
    let dir = exe.parent().ok_or("binário sem pasta")?;
    let staged = dir.join(".bitclaw.update");
    fs::copy(&fresh, &staged).map_err(|e| format!("não consegui escrever em {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    fs::rename(&staged, exe).map_err(|e| {
        let _ = fs::remove_file(&staged);
        format!("não consegui trocar {}: {e}", exe.display())
    })
}

fn work_dir() -> PathBuf {
    std::env::temp_dir().join(format!("bitclaw-update-{}", std::process::id()))
}

// O comando inteiro. Devolve o texto para o terminal.
pub fn run() -> Result<String, String> {
    let target = target().ok_or_else(|| {
        format!("não há release para {} {}; compile com cargo install --git {REPOSITORY}", std::env::consts::OS, std::env::consts::ARCH)
    })?;
    let tag = latest_tag(Duration::from_secs(15))?;
    if !is_newer(&tag, VERSION) {
        return Ok(format!("bitclaw {VERSION} já é a versão mais nova (último release: {tag})."));
    }
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|e| format!("não achei o próprio binário: {e}"))?;
    let asset = asset_name(target);
    let base = format!("{}/{tag}", download_base());
    let sums = String::from_utf8(download(&format!("{base}/SHA256SUMS"))?).map_err(|e| e.to_string())?;
    let archive = download(&format!("{base}/{asset}"))?;
    let work = work_dir();
    let result = install(&archive, &sums, &asset, &exe, &work);
    let _ = fs::remove_dir_all(&work);
    result?;
    Ok(format!("bitclaw atualizado de {VERSION} para {tag} em {}.", exe.display()))
}

// ---- Aviso de versão nova ----
//
// A abertura nunca espera a rede: o aviso vem de um cache. Se o cache tem mais de um dia, uma thread
// consulta o GitHub em segundo plano e grava a resposta, que aparece na próxima abertura.

const CHECK_FILE: &str = "update-check.json";
const CHECK_EVERY_SECS: u64 = 24 * 60 * 60;

// Pasta do estado global do BitClaw (fora de qualquer projeto): `$XDG_CONFIG_HOME/bitclaw` ou
// `~/.config/bitclaw`. `None` sem `HOME`.
pub fn config_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(x) => PathBuf::from(x),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("bitclaw"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

// O que o cache diz agora: o aviso a mostrar (se houver) e se está na hora de consultar de novo.
fn read_check(cache: Option<&str>, now: u64, current: &str) -> (Option<String>, bool) {
    let v: serde_json::Value = cache.and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    // Sem data legível (sem cache, ou estragado), consulta já.
    let due = v["checked"].as_u64().is_none_or(|checked| now.saturating_sub(checked) >= CHECK_EVERY_SECS);
    let notice = v["latest"]
        .as_str()
        .filter(|latest| is_newer(latest, current))
        .map(|latest| format!("versão {latest} disponível (você tem {current}): rode bitclaw update"));
    (notice, due)
}

// Aviso de versão nova para o cabeçalho. Desligado em build de desenvolvimento e com
// `BITCLAW_NO_UPDATE_CHECK`, porque é uma chamada de rede que o usuário não pediu.
pub fn startup_notice() -> Option<String> {
    if cfg!(debug_assertions) || std::env::var_os("BITCLAW_NO_UPDATE_CHECK").is_some() || target().is_none() {
        return None;
    }
    let path = config_dir()?.join(CHECK_FILE);
    let (notice, due) = read_check(fs::read_to_string(&path).ok().as_deref(), now_secs(), VERSION);
    if due {
        std::thread::spawn(move || {
            // Sem resposta (offline, sem release), grava só a hora: tenta de novo amanhã, não a cada abertura.
            let latest = latest_tag(Duration::from_secs(4)).ok();
            let body = serde_json::json!({ "checked": now_secs(), "latest": latest });
            if let Some(dir) = path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let _ = fs::write(&path, body.to_string());
        });
    }
    notice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_comes_from_the_cache_and_the_check_runs_once_a_day() {
        let day = CHECK_EVERY_SECS;
        assert_eq!(read_check(None, 10 * day, "0.1.0"), (None, true), "sem cache: consulta");
        let fresh = r#"{"checked": 1000, "latest": "v0.2.0"}"#;
        let (notice, due) = read_check(Some(fresh), 1000 + 60, "0.1.0");
        assert_eq!(notice.as_deref(), Some("versão v0.2.0 disponível (você tem 0.1.0): rode bitclaw update"));
        assert!(!due, "consultou há um minuto");
        assert!(read_check(Some(fresh), 1000 + day, "0.1.0").1, "um dia depois, consulta de novo");
        assert_eq!(read_check(Some(fresh), 1060, "0.2.0").0, None, "já atualizado: sem aviso");
        assert_eq!(read_check(Some(r#"{"checked": 1000, "latest": null}"#), 1060, "0.1.0"), (None, false), "offline");
        assert_eq!(read_check(Some("lixo"), 1060, "0.1.0"), (None, true), "cache estragado: consulta");
    }

    #[test]
    fn config_dir_follows_xdg_or_home() {
        // Só o formato: as variáveis do processo são compartilhadas entre testes.
        if let Some(dir) = config_dir() {
            assert!(dir.ends_with("bitclaw"), "{}", dir.display());
        }
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bitclaw-update-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    // Um release falso: `bitclaw` com `content`, empacotado com o tar do sistema.
    fn archive(dir: &Path, content: &str) -> Vec<u8> {
        let src = dir.join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("bitclaw"), content).unwrap();
        let out = dir.join("a.tar.gz");
        assert!(Command::new("tar").arg("-czf").arg(&out).arg("-C").arg(&src).arg("bitclaw").status().unwrap().success());
        fs::read(out).unwrap()
    }

    #[test]
    fn versions_compare_by_number_and_ignore_what_they_cannot_read() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"), "número, não texto");
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("v0.2.0-rc.1", "0.1.0"), "pré-release não conta");
        assert!(!is_newer("nightly", "0.1.0"));
    }

    #[test]
    fn the_sum_is_found_by_asset_name() {
        let sums = "aaa  bitclaw-x86_64-unknown-linux-gnu.tar.gz\nbbb *bitclaw-aarch64-apple-darwin.tar.gz\n";
        assert_eq!(expected_sum(sums, "bitclaw-aarch64-apple-darwin.tar.gz"), Some("bbb"));
        assert_eq!(expected_sum(sums, "bitclaw-x86_64-unknown-linux-gnu.tar.gz"), Some("aaa"));
        assert_eq!(expected_sum(sums, "bitclaw-outro.tar.gz"), None);
    }

    #[test]
    fn this_platform_has_an_asset_name() {
        // Os testes rodam nas plataformas do release (macOS e Linux, arm64 e x86_64).
        let t = target().expect("plataforma sem release");
        assert!(asset_name(t).starts_with("bitclaw-") && asset_name(t).ends_with(".tar.gz"));
        assert_eq!(slug(), "Gabrielfernandes7/bitclaw");
    }

    #[test]
    fn install_swaps_the_binary_when_the_sum_matches() {
        let dir = tempdir("ok");
        let data = archive(&dir, "#!/bin/sh\necho nova\n");
        let exe = dir.join("bin/bitclaw");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, "velha").unwrap();
        let sums = format!("{}  bitclaw-t.tar.gz\n", sha256_hex(&data));
        install(&data, &sums, "bitclaw-t.tar.gz", &exe, &dir.join("work")).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "#!/bin/sh\necho nova\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o111, 0o111, "executável");
        }
        assert!(!dir.join("bin/.bitclaw.update").exists(), "não deixa o arquivo temporário");
    }

    #[test]
    fn install_refuses_a_wrong_sum_and_keeps_the_old_binary() {
        let dir = tempdir("bad");
        let data = archive(&dir, "nova");
        let exe = dir.join("bitclaw");
        fs::write(&exe, "velha").unwrap();
        let err = install(&data, &format!("{}  bitclaw-t.tar.gz", "0".repeat(64)), "bitclaw-t.tar.gz", &exe, &dir.join("w")).unwrap_err();
        assert!(err.contains("não confere") && err.contains("Nada foi trocado"), "{err}");
        assert_eq!(fs::read_to_string(&exe).unwrap(), "velha");
        let missing = install(&data, "", "bitclaw-t.tar.gz", &exe, &dir.join("w")).unwrap_err();
        assert!(missing.contains("SHA256SUMS não tem"), "{missing}");
    }
}

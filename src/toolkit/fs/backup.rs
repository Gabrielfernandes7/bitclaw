//! Backup do que a ferramenta vai sobrescrever ou remover, em `.bitclaw/backup`.

use super::BACKUP_DIR;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Guarda `content` como `<caminho com / trocado por _>.<timestamp>.bak`.
pub(super) fn backup(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let backup_name = rel.replace(['/', '\\'], "_");
    let backup_dir = root.join(BACKUP_DIR);
    fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
    let backup_path = backup_dir.join(format!("{backup_name}.{ts}.bak"));
    fs::write(&backup_path, content).map_err(|e| e.to_string())
}

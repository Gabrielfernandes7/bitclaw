//! Recibos de aprovação: cada decisão sobre uma alteração ou execução de script vai para
//! `.bitclaw/approvals.jsonl`, com o tipo, o título da prévia e quem decidiu. Só registra: a decisão é
//! a do `Approver` embrulhado. Aprovação em lote (`a` na interface) aparece como do usuário, porque a
//! resposta chega pela interface, e não há como distinguir daqui.

use bitclaw::tools::{Approver, Preview, PreviewKind};
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECEIPTS_FILE: &str = ".bitclaw/approvals.jsonl";

pub struct Recorded {
    inner: Box<dyn Approver>,
    path: PathBuf,
    // `--yes`: as alterações passam sem perguntar. Execuções de script continuam perguntando.
    auto_edits: bool,
}

impl Recorded {
    pub fn new(inner: Box<dyn Approver>, root: &Path, auto_edits: bool) -> Self {
        Self { inner, path: root.join(RECEIPTS_FILE), auto_edits }
    }

    fn record(&self, preview: &Preview, run: bool, approved: bool, by: &str) {
        let kind = match (run, preview.kind) {
            (true, _) => "execução",
            (false, PreviewKind::Change) => "alteração",
            (false, PreviewKind::Removal) => "remoção",
        };
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let line = json!({
            "ts": ts,
            "tipo": kind,
            "titulo": preview.title,
            "decisao": if approved { "aprovada" } else { "recusada" },
            "por": by,
        });
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

impl Approver for Recorded {
    fn approve(&self, preview: &Preview) -> bool {
        let approved = self.inner.approve(preview);
        self.record(preview, false, approved, if self.auto_edits { "--yes" } else { "usuário" });
        approved
    }

    fn approve_run(&self, preview: &Preview) -> bool {
        let approved = self.inner.approve_run(preview);
        self.record(preview, true, approved, "usuário");
        approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitclaw::tools::{AutoApprover, PreviewLine};

    fn root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bitclaw-receipts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d.canonicalize().unwrap()
    }

    #[test]
    fn every_decision_is_written_with_its_kind_and_who_decided() {
        let r = root("decisions");
        let change = Preview::new("edição proposta em a.md", vec![PreviewLine::Added("x".into())]);
        let removal = Preview::removal("remover arquivo b.md", Vec::new());
        let script = Preview::new("executar s.sh", Vec::new());

        Recorded::new(Box::new(AutoApprover), &r, true).approve(&change);
        Recorded::new(Box::new(AutoApprover), &r, false).approve(&removal);
        Recorded::new(Box::new(AutoApprover), &r, true).approve_run(&script);

        let text = fs::read_to_string(r.join(RECEIPTS_FILE)).unwrap();
        let lines: Vec<serde_json::Value> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0]["tipo"].as_str(), lines[0]["por"].as_str()), (Some("alteração"), Some("--yes")));
        assert_eq!((lines[1]["tipo"].as_str(), lines[1]["por"].as_str()), (Some("remoção"), Some("usuário")));
        // AutoApprover recusa execução de script: a decisão registrada é a recusa.
        assert_eq!((lines[2]["tipo"].as_str(), lines[2]["decisao"].as_str()), (Some("execução"), Some("recusada")));
    }
}

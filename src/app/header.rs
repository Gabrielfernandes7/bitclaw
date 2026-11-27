//! Texto do cabeçalho: a linha do projeto e a do modelo com o esforço e o provedor.

use bitclaw::models::Effort;
use std::path::Path;

// `Projeto (ClawBit) /caminho/do/projeto`: o nome da pasta e o caminho inteiro.
pub fn project_line(root: &Path) -> String {
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.display().to_string());
    format!("Projeto ({name}) {}", root.display())
}

// O que o esforço diz no cabeçalho: "with high effort"; sem raciocínio, "thinking off".
fn effort_phrase(effort: Effort) -> String {
    match effort {
        Effort::Off => "thinking off".into(),
        Effort::On => "thinking on".into(),
        level => format!("with {} effort", level.as_str()),
    }
}

// `nemotron:e4b with high effort · Ollama · 9.0B · nvfp4 · janela 16384 tok`. `details` são os dados do
// modelo que o provedor soube dizer (vazio se não sabe); `window` é a janela de contexto em uso.
pub fn model_line(model: &str, effort: Effort, provider: &str, details: &str, window: u32) -> String {
    let mut parts = vec![format!("{model} {}", effort_phrase(effort))];
    if !provider.is_empty() {
        parts.push(provider.to_string());
    }
    if !details.is_empty() {
        parts.push(details.to_string());
    }
    parts.push(format!("janela {window} tok"));
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_project_line_names_the_folder_and_the_path() {
        assert_eq!(project_line(Path::new("/Users/a/ClawBit")), "Projeto (ClawBit) /Users/a/ClawBit");
        assert_eq!(project_line(Path::new("/")), "Projeto (/) /");
    }

    #[test]
    fn the_model_line_says_the_effort_and_the_provider() {
        let line = model_line("e4b", Effort::High, "Ollama", "9.0B · nvfp4", 16384);
        assert_eq!(line, "e4b with high effort · Ollama · 9.0B · nvfp4 · janela 16384 tok");
        assert_eq!(model_line("m", Effort::Off, "", "", 8192), "m thinking off · janela 8192 tok");
        assert_eq!(model_line("m", Effort::On, "OpenRouter", "", 8192), "m thinking on · OpenRouter · janela 8192 tok");
        assert!(model_line("m", Effort::Medium, "x", "", 1).starts_with("m with medium effort"));
    }
}

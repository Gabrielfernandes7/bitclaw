//! Ferramentas de arquivo: list_dir, read_file, edit_file, create_file, create_dir e delete_file.

mod backup;
mod create_dir;
mod create_file;
mod delete_file;
mod edit_file;
mod list_dir;
mod preview;
mod read_file;

pub use create_dir::CreateDir;
pub use create_file::CreateFile;
pub use delete_file::DeleteFile;
pub use edit_file::EditFile;
pub use list_dir::ListDir;
pub use read_file::ReadFile;

pub(crate) const BACKUP_DIR: &str = ".bitclaw/backup";

#[cfg(test)]
mod tests;

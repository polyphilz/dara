use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabasePaths {
    pub root: PathBuf,
    pub main: PathBuf,
    pub media: PathBuf,
    pub backups: PathBuf,
}

impl DatabasePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            main: root.join("dara.sqlite3"),
            media: root.join("media.sqlite3"),
            backups: root.join("backups"),
            root,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

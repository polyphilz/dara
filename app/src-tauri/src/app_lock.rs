use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
};

use tauri::{plugin::TauriPlugin, Manager, Runtime};

const APP_DATA_LOCK_FILE_NAME: &str = ".dara.lock";

#[derive(Debug, thiserror::Error)]
pub enum AppDataLockError {
    #[error("Dara data directory is already in use: {0}")]
    AlreadyLocked(PathBuf),

    #[error("could not lock Dara data directory: {0}")]
    Io(#[from] std::io::Error),

    #[error("Dara Local requires an explicit DARA_DATA_DIR")]
    MissingDevelopmentDataDirectory,
}

pub struct AppDataLock {
    data_root: PathBuf,
    _file: File,
}

impl AppDataLock {
    pub fn acquire(data_root: &Path) -> Result<Self, AppDataLockError> {
        fs::create_dir_all(data_root)?;
        let lock_path = data_root.join(APP_DATA_LOCK_FILE_NAME);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                data_root: data_root.to_owned(),
                _file: file,
            }),
            Err(TryLockError::WouldBlock) => Err(AppDataLockError::AlreadyLocked(lock_path)),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }

    pub fn data_root(&self) -> &Path {
        &self.data_root
    }
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("app-data-lock")
        .setup(|app, _api| {
            let data_root = match std::env::var_os("DARA_DATA_DIR") {
                Some(path) => PathBuf::from(path),
                None if is_production_build() => app.path().data_dir()?.join("dara"),
                None => return Err(AppDataLockError::MissingDevelopmentDataDirectory.into()),
            };
            app.manage(AppDataLock::acquire(&data_root)?);
            Ok(())
        })
        .build()
}

fn is_production_build() -> bool {
    env!("DARA_PRODUCTION_BUILD") == "true"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_excludes_a_second_data_directory_owner() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let first = AppDataLock::acquire(directory.path()).expect("first lock");

        assert!(matches!(
            AppDataLock::acquire(directory.path()),
            Err(AppDataLockError::AlreadyLocked(_))
        ));

        drop(first);
        AppDataLock::acquire(directory.path()).expect("lock after release");
    }

    #[test]
    fn packaged_identifier_and_production_boundary_agree() {
        assert_eq!(
            is_production_build(),
            env!("DARA_APP_IDENTIFIER") == "com.rohan.dara"
        );
    }
}

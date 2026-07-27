use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::domain::InstallationId;

const INSTALLATION_ID_FILE: &str = "installation-id-v1.json";
const INSTALLATION_FORMAT_VERSION: u32 = 1;
const MAX_INSTALLATION_FILE_BYTES: u64 = 4 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct InstallationIdentityStore {
    path: PathBuf,
}

impl InstallationIdentityStore {
    pub(crate) fn new(data_root: &Path) -> Result<Self, InstallationIdentityError> {
        if !data_root.is_absolute() {
            return Err(InstallationIdentityError::RelativeDataRoot);
        }
        Ok(Self {
            path: data_root.join(INSTALLATION_ID_FILE),
        })
    }

    pub(crate) fn load_or_create(&self) -> Result<InstallationId, InstallationIdentityError> {
        match self.load() {
            Ok(installation_id) => Ok(installation_id),
            Err(InstallationIdentityError::Io(error))
                if error.kind() == std::io::ErrorKind::NotFound =>
            {
                self.create()
            }
            Err(error) => Err(error),
        }
    }

    fn create(&self) -> Result<InstallationId, InstallationIdentityError> {
        self.create_with(|file, bytes| file.write_all(bytes).and_then(|()| file.sync_all()))
    }

    fn create_with(
        &self,
        write: impl FnOnce(&mut File, &[u8]) -> std::io::Result<()>,
    ) -> Result<InstallationId, InstallationIdentityError> {
        let installation_id = InstallationId::new();
        let temporary = self.path.with_file_name(format!(
            ".{INSTALLATION_ID_FILE}.{}.tmp",
            installation_id.as_str()
        ));
        let outcome = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(InstallationIdentityError::Io)?;
            set_private_permissions(&file)?;
            let payload = InstallationIdentityFile {
                format_version: INSTALLATION_FORMAT_VERSION,
                installation_id: installation_id.clone(),
            };
            let bytes =
                serde_json::to_vec(&payload).map_err(|_| InstallationIdentityError::InvalidFile)?;
            write(&mut file, &bytes).map_err(InstallationIdentityError::Io)?;
            drop(file);

            // A hard link publishes the synced file atomically without replacing
            // an identity concurrently created by another caller.
            match fs::hard_link(&temporary, &self.path) {
                Ok(()) => Ok(installation_id),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => self.load(),
                Err(error) => Err(InstallationIdentityError::Io(error)),
            }
        })();
        match fs::remove_file(&temporary) {
            Ok(()) => outcome,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => outcome,
            Err(error) if outcome.is_ok() => Err(InstallationIdentityError::Io(error)),
            Err(_) => outcome,
        }
    }

    pub(crate) fn load(&self) -> Result<InstallationId, InstallationIdentityError> {
        let metadata = fs::symlink_metadata(&self.path).map_err(InstallationIdentityError::Io)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_INSTALLATION_FILE_BYTES
        {
            return Err(InstallationIdentityError::InvalidFile);
        }
        verify_private_permissions(&metadata)?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        File::open(&self.path)
            .and_then(|mut file| file.read_to_end(&mut bytes))
            .map_err(InstallationIdentityError::Io)?;
        let payload: InstallationIdentityFile =
            serde_json::from_slice(&bytes).map_err(|_| InstallationIdentityError::InvalidFile)?;
        if payload.format_version != INSTALLATION_FORMAT_VERSION {
            return Err(InstallationIdentityError::UnsupportedVersion);
        }
        Ok(payload.installation_id)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstallationIdentityFile {
    format_version: u32,
    installation_id: InstallationId,
}

#[cfg(unix)]
fn set_private_permissions(file: &File) -> Result<(), InstallationIdentityError> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(InstallationIdentityError::Io)
}

#[cfg(not(unix))]
fn set_private_permissions(_file: &File) -> Result<(), InstallationIdentityError> {
    Ok(())
}

#[cfg(unix)]
fn verify_private_permissions(metadata: &fs::Metadata) -> Result<(), InstallationIdentityError> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(InstallationIdentityError::UnsafePermissions);
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_private_permissions(_metadata: &fs::Metadata) -> Result<(), InstallationIdentityError> {
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum InstallationIdentityError {
    #[error("installation identity requires an absolute Dara data root")]
    RelativeDataRoot,
    #[error("installation identity file I/O failed")]
    Io(#[source] std::io::Error),
    #[error("installation identity file is invalid")]
    InvalidFile,
    #[error("installation identity file uses an unsupported version")]
    UnsupportedVersion,
    #[error("installation identity file permissions are unsafe")]
    UnsafePermissions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_identity_is_stable_private_and_outside_the_database_pair() {
        let directory = tempfile::tempdir().expect("data root");
        let store = InstallationIdentityStore::new(directory.path()).expect("store");
        let first = store.load_or_create().expect("create identity");
        let second = store.load_or_create().expect("load identity");
        assert_eq!(first, second);
        assert_eq!(
            store.path().file_name().and_then(|value| value.to_str()),
            Some(INSTALLATION_ID_FILE)
        );
        assert_ne!(
            store.path().file_name().and_then(|value| value.to_str()),
            Some("dara.sqlite3")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.path())
                    .expect("identity metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn installation_identity_rejects_relative_roots_and_unsafe_files() {
        assert!(matches!(
            InstallationIdentityStore::new(Path::new("relative")),
            Err(InstallationIdentityError::RelativeDataRoot)
        ));
        let directory = tempfile::tempdir().expect("data root");
        let store = InstallationIdentityStore::new(directory.path()).expect("store");
        fs::write(store.path(), b"{}").expect("invalid identity");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(store.path(), fs::Permissions::from_mode(0o600))
                .expect("private permissions");
        }
        assert!(matches!(
            store.load(),
            Err(InstallationIdentityError::InvalidFile)
        ));
    }

    #[test]
    fn failed_identity_write_does_not_poison_the_final_path() {
        let directory = tempfile::tempdir().expect("data root");
        let store = InstallationIdentityStore::new(directory.path()).expect("store");
        let error = store
            .create_with(|file, bytes| {
                file.write_all(&bytes[..1])?;
                Err(std::io::Error::other("injected write failure"))
            })
            .expect_err("injected failure");
        assert!(matches!(error, InstallationIdentityError::Io(_)));
        assert!(!store.path().exists());

        let created = store
            .load_or_create()
            .expect("retry creates a valid identity");
        assert_eq!(store.load().expect("load retried identity"), created);
        let remaining_files = fs::read_dir(directory.path())
            .expect("list data root")
            .collect::<std::io::Result<Vec<_>>>()
            .expect("data-root entries");
        assert_eq!(remaining_files.len(), 1);
    }
}

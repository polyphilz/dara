use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::domain::BackupSetId;

const KEYCHAIN_SERVICE_SUFFIX: &str = ".offsite-backup.r2";
const LEGACY_APP_IDENTIFIERS_SEPARATOR: char = ',';
const CREDENTIAL_FORMAT_VERSION: u32 = 1;
const MAX_CREDENTIAL_PAYLOAD_BYTES: usize = 4 * 1024;

pub(crate) struct R2Credentials {
    access_key_id: Zeroizing<String>,
    secret_access_key: Zeroizing<String>,
}

impl R2Credentials {
    pub(crate) fn new(
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Result<Self, CredentialError> {
        let mut access_key_id = access_key_id.into();
        let mut secret_access_key = secret_access_key.into();
        if access_key_id.len() != 32 || !is_lower_hex(&access_key_id) {
            access_key_id.zeroize();
            secret_access_key.zeroize();
            return Err(CredentialError::InvalidCredential("accessKeyId"));
        }
        if secret_access_key.len() != 64 || !is_lower_hex(&secret_access_key) {
            access_key_id.zeroize();
            secret_access_key.zeroize();
            return Err(CredentialError::InvalidCredential("secretAccessKey"));
        }
        Ok(Self {
            access_key_id: Zeroizing::new(access_key_id),
            secret_access_key: Zeroizing::new(secret_access_key),
        })
    }

    pub(crate) fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    pub(crate) fn secret_access_key(&self) -> &str {
        &self.secret_access_key
    }

    pub(crate) fn try_clone(&self) -> Result<Self, CredentialError> {
        Self::new(
            self.access_key_id().to_owned(),
            self.secret_access_key().to_owned(),
        )
    }

    fn encode(&self) -> Result<Zeroizing<Vec<u8>>, CredentialError> {
        let payload = CredentialPayloadRef {
            format_version: CREDENTIAL_FORMAT_VERSION,
            access_key_id: self.access_key_id(),
            secret_access_key: self.secret_access_key(),
        };
        let bytes = serde_json::to_vec(&payload).map_err(|_| CredentialError::CorruptPayload)?;
        if bytes.len() > MAX_CREDENTIAL_PAYLOAD_BYTES {
            return Err(CredentialError::CorruptPayload);
        }
        Ok(Zeroizing::new(bytes))
    }

    fn decode(bytes: &[u8]) -> Result<Self, CredentialError> {
        if bytes.len() > MAX_CREDENTIAL_PAYLOAD_BYTES {
            return Err(CredentialError::CorruptPayload);
        }
        let mut payload: CredentialPayload =
            serde_json::from_slice(bytes).map_err(|_| CredentialError::CorruptPayload)?;
        if payload.format_version != CREDENTIAL_FORMAT_VERSION {
            payload.access_key_id.zeroize();
            payload.secret_access_key.zeroize();
            return Err(CredentialError::UnsupportedPayloadVersion);
        }
        let credentials = Self::new(
            std::mem::take(&mut payload.access_key_id),
            std::mem::take(&mut payload.secret_access_key),
        );
        payload.access_key_id.zeroize();
        payload.secret_access_key.zeroize();
        credentials
    }
}

impl fmt::Debug for R2Credentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("R2Credentials")
            .field("access_key_id", &"[REDACTED]")
            .field("secret_access_key", &"[REDACTED]")
            .finish()
    }
}

fn is_lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CredentialPayloadRef<'a> {
    format_version: u32,
    access_key_id: &'a str,
    secret_access_key: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CredentialPayload {
    format_version: u32,
    access_key_id: String,
    secret_access_key: String,
}

pub(crate) trait CredentialStore: Send + Sync {
    fn save(
        &self,
        backup_set_id: &BackupSetId,
        credentials: &R2Credentials,
    ) -> Result<(), CredentialError>;
    fn load(&self, backup_set_id: &BackupSetId) -> Result<R2Credentials, CredentialError>;
    fn remove(&self, backup_set_id: &BackupSetId) -> Result<(), CredentialError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct MacOsKeychainCredentialStore;

#[cfg(target_os = "macos")]
trait KeychainBackend {
    fn load(&self, service: &str, account: &str) -> Result<Vec<u8>, CredentialError>;
    fn save(&self, service: &str, account: &str, payload: &[u8]) -> Result<(), CredentialError>;
    fn remove(&self, service: &str, account: &str) -> Result<(), CredentialError>;
}

#[cfg(target_os = "macos")]
static KEYCHAIN_OPERATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "macos")]
fn keychain_operation_guard() -> std::sync::MutexGuard<'static, ()> {
    KEYCHAIN_OPERATION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn keychain_service() -> String {
    keychain_service_for_identifier(env!("DARA_APP_IDENTIFIER"))
}

fn legacy_keychain_services() -> Vec<String> {
    env!("DARA_LEGACY_APP_IDENTIFIERS")
        .split(LEGACY_APP_IDENTIFIERS_SEPARATOR)
        .filter(|identifier| !identifier.is_empty())
        .map(keychain_service_for_identifier)
        .collect()
}

fn keychain_service_for_identifier(identifier: &str) -> String {
    format!("{identifier}{KEYCHAIN_SERVICE_SUFFIX}")
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug, Default)]
struct SystemKeychainBackend;

#[cfg(target_os = "macos")]
impl SystemKeychainBackend {
    fn entry(service: &str, account: &str) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(service, account).map_err(|_| CredentialError::Unavailable)
    }
}

#[cfg(target_os = "macos")]
impl KeychainBackend for SystemKeychainBackend {
    fn load(&self, service: &str, account: &str) -> Result<Vec<u8>, CredentialError> {
        Self::entry(service, account)?
            .get_secret()
            .map_err(map_keyring_load_error)
    }

    fn save(&self, service: &str, account: &str, payload: &[u8]) -> Result<(), CredentialError> {
        Self::entry(service, account)?
            .set_secret(payload)
            .map_err(|_| CredentialError::Unavailable)
    }

    fn remove(&self, service: &str, account: &str) -> Result<(), CredentialError> {
        match Self::entry(service, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialError::Unavailable),
        }
    }
}

#[cfg(target_os = "macos")]
fn load_credentials(
    backend: &impl KeychainBackend,
    backup_set_id: &BackupSetId,
) -> Result<R2Credentials, CredentialError> {
    let account = backup_set_id.as_str();
    let current_service = keychain_service();
    match backend.load(&current_service, account) {
        Ok(payload) => {
            let credentials = decode_keychain_payload(payload)?;
            remove_legacy_credentials_best_effort(backend, account);
            Ok(credentials)
        }
        Err(CredentialError::Missing) => {
            migrate_legacy_credentials(backend, account, &current_service)
        }
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn migrate_legacy_credentials(
    backend: &impl KeychainBackend,
    account: &str,
    current_service: &str,
) -> Result<R2Credentials, CredentialError> {
    for legacy_service in legacy_keychain_services() {
        let payload = match backend.load(&legacy_service, account) {
            Ok(payload) => payload,
            Err(CredentialError::Missing) => continue,
            Err(error) => return Err(error),
        };
        let mut payload = Zeroizing::new(payload);
        let credentials = R2Credentials::decode(&payload)?;
        backend.save(current_service, account, &payload)?;
        let mut verified = Zeroizing::new(backend.load(current_service, account)?);
        if verified.as_slice() != payload.as_slice() {
            let cleanup_result = backend.remove(current_service, account);
            payload.zeroize();
            verified.zeroize();
            cleanup_result?;
            return Err(CredentialError::Unavailable);
        }
        let _ = backend.remove(&legacy_service, account);
        return Ok(credentials);
    }
    backend
        .load(current_service, account)
        .and_then(decode_keychain_payload)
}

#[cfg(target_os = "macos")]
fn decode_keychain_payload(mut payload: Vec<u8>) -> Result<R2Credentials, CredentialError> {
    let credentials = R2Credentials::decode(&payload);
    payload.zeroize();
    credentials
}

#[cfg(target_os = "macos")]
fn remove_legacy_credentials_best_effort(backend: &impl KeychainBackend, account: &str) {
    for legacy_service in legacy_keychain_services() {
        let _ = backend.remove(&legacy_service, account);
    }
}

#[cfg(target_os = "macos")]
fn remove_all_credentials(
    backend: &impl KeychainBackend,
    backup_set_id: &BackupSetId,
) -> Result<(), CredentialError> {
    let account = backup_set_id.as_str();
    let services = std::iter::once(keychain_service()).chain(legacy_keychain_services());
    let mut unavailable = false;
    for service in services {
        if backend.remove(&service, account).is_err() {
            unavailable = true;
        }
    }
    if unavailable {
        Err(CredentialError::Unavailable)
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
impl CredentialStore for MacOsKeychainCredentialStore {
    fn save(
        &self,
        backup_set_id: &BackupSetId,
        credentials: &R2Credentials,
    ) -> Result<(), CredentialError> {
        let _operation_guard = keychain_operation_guard();
        let payload = credentials.encode()?;
        let backend = SystemKeychainBackend;
        backend.save(
            &keychain_service(),
            backup_set_id.as_str(),
            payload.as_slice(),
        )?;
        remove_legacy_credentials_best_effort(&backend, backup_set_id.as_str());
        Ok(())
    }

    fn load(&self, backup_set_id: &BackupSetId) -> Result<R2Credentials, CredentialError> {
        let _operation_guard = keychain_operation_guard();
        load_credentials(&SystemKeychainBackend, backup_set_id)
    }

    fn remove(&self, backup_set_id: &BackupSetId) -> Result<(), CredentialError> {
        let _operation_guard = keychain_operation_guard();
        remove_all_credentials(&SystemKeychainBackend, backup_set_id)
    }
}

#[cfg(target_os = "macos")]
fn map_keyring_load_error(error: keyring::Error) -> CredentialError {
    match error {
        keyring::Error::NoEntry => CredentialError::Missing,
        _ => CredentialError::Unavailable,
    }
}

#[cfg(not(target_os = "macos"))]
impl CredentialStore for MacOsKeychainCredentialStore {
    fn save(
        &self,
        _backup_set_id: &BackupSetId,
        _credentials: &R2Credentials,
    ) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }

    fn load(&self, _backup_set_id: &BackupSetId) -> Result<R2Credentials, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    fn remove(&self, _backup_set_id: &BackupSetId) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CredentialError {
    #[error("invalid R2 credential field: {0}")]
    InvalidCredential(&'static str),
    #[error("off-site backup credentials are missing")]
    Missing,
    #[error("macOS Keychain is unavailable for off-site backup")]
    Unavailable,
    #[error("saved off-site backup credentials use an unsupported version")]
    UnsupportedPayloadVersion,
    #[error("saved off-site backup credentials are invalid")]
    CorruptPayload,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use std::{
        collections::HashMap,
        sync::{Mutex, MutexGuard},
    };

    const ACCESS_KEY: &str = "0123456789abcdef0123456789abcdef";
    const SECRET_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn credential_payload_is_versioned_and_debug_is_redacted() {
        let credentials = R2Credentials::new(ACCESS_KEY, SECRET_KEY).expect("credentials");
        let payload = credentials.encode().expect("payload");
        let decoded = R2Credentials::decode(&payload).expect("decoded");
        assert_eq!(decoded.access_key_id(), ACCESS_KEY);
        assert_eq!(decoded.secret_access_key(), SECRET_KEY);
        let debug = format!("{credentials:?}");
        assert!(!debug.contains(ACCESS_KEY));
        assert!(!debug.contains(SECRET_KEY));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn keychain_service_is_scoped_to_the_packaged_application_identity() {
        assert_eq!(
            keychain_service(),
            format!("{}{KEYCHAIN_SERVICE_SUFFIX}", env!("DARA_APP_IDENTIFIER"))
        );
        assert_eq!(
            legacy_keychain_services(),
            env!("DARA_LEGACY_APP_IDENTIFIERS")
                .split(LEGACY_APP_IDENTIFIERS_SEPARATOR)
                .filter(|identifier| !identifier.is_empty())
                .map(keychain_service_for_identifier)
                .collect::<Vec<_>>()
        );
        assert!(!legacy_keychain_services().contains(&keychain_service()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn legacy_keychain_credentials_migrate_and_are_verified_before_cleanup() {
        let backend = FakeKeychainBackend::default();
        let backup_set_id = BackupSetId::new();
        let legacy_service = legacy_keychain_services()
            .into_iter()
            .next()
            .expect("legacy service");
        let credentials = R2Credentials::new(ACCESS_KEY, SECRET_KEY).expect("credentials");
        backend.seed(
            &legacy_service,
            backup_set_id.as_str(),
            credentials.encode().expect("payload").as_slice(),
        );

        let migrated = load_credentials(&backend, &backup_set_id).expect("migrated credentials");

        assert_eq!(migrated.access_key_id(), ACCESS_KEY);
        assert_eq!(migrated.secret_access_key(), SECRET_KEY);
        assert!(backend.contains(&keychain_service(), backup_set_id.as_str()));
        assert!(!backend.contains(&legacy_service, backup_set_id.as_str()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn failed_keychain_migration_preserves_legacy_credentials() {
        let backend = FakeKeychainBackend::default();
        let backup_set_id = BackupSetId::new();
        let current_service = keychain_service();
        let legacy_service = legacy_keychain_services()
            .into_iter()
            .next()
            .expect("legacy service");
        let credentials = R2Credentials::new(ACCESS_KEY, SECRET_KEY).expect("credentials");
        backend.seed(
            &legacy_service,
            backup_set_id.as_str(),
            credentials.encode().expect("payload").as_slice(),
        );
        backend.corrupt_reads_for(&current_service, backup_set_id.as_str());

        assert!(matches!(
            load_credentials(&backend, &backup_set_id),
            Err(CredentialError::Unavailable)
        ));
        assert!(backend.contains(&legacy_service, backup_set_id.as_str()));
        assert!(!backend.contains(&current_service, backup_set_id.as_str()));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn lost_keychain_migration_race_rechecks_current_credentials() {
        let backup_set_id = BackupSetId::new();
        let current_service = keychain_service();
        let legacy_service = legacy_keychain_services()
            .into_iter()
            .next()
            .expect("legacy service");
        let credentials = R2Credentials::new(ACCESS_KEY, SECRET_KEY).expect("credentials");
        let backend = MigrationRaceBackend {
            current_service,
            legacy_service,
            account: backup_set_id.as_str().to_owned(),
            payload: credentials.encode().expect("payload").to_vec(),
            migration_completed: Mutex::new(false),
        };

        let loaded = load_credentials(&backend, &backup_set_id).expect("current credentials");

        assert_eq!(loaded.access_key_id(), ACCESS_KEY);
        assert_eq!(loaded.secret_access_key(), SECRET_KEY);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn current_keychain_credentials_win_and_remove_cleans_every_service() {
        let backend = FakeKeychainBackend::default();
        let backup_set_id = BackupSetId::new();
        let current = R2Credentials::new(ACCESS_KEY, SECRET_KEY).expect("current credentials");
        backend.seed(
            &keychain_service(),
            backup_set_id.as_str(),
            current.encode().expect("current payload").as_slice(),
        );
        for service in legacy_keychain_services() {
            backend.seed(
                &service,
                backup_set_id.as_str(),
                current.encode().expect("legacy payload").as_slice(),
            );
        }

        let loaded = load_credentials(&backend, &backup_set_id).expect("current credentials");

        assert_eq!(loaded.access_key_id(), ACCESS_KEY);
        assert_eq!(loaded.secret_access_key(), SECRET_KEY);
        for service in legacy_keychain_services() {
            assert!(!backend.contains(&service, backup_set_id.as_str()));
        }

        remove_all_credentials(&backend, &backup_set_id).expect("remove credentials");
        assert!(!backend.contains(&keychain_service(), backup_set_id.as_str()));
    }

    #[test]
    fn malformed_credentials_and_payloads_fail_closed() {
        assert!(R2Credentials::new("short", SECRET_KEY).is_err());
        assert!(R2Credentials::new(ACCESS_KEY, "short").is_err());
        assert!(R2Credentials::decode(
            br#"{"formatVersion":2,"accessKeyId":"0123456789abcdef0123456789abcdef","secretAccessKey":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}"#
        )
        .is_err());
        assert!(R2Credentials::decode(
            br#"{"formatVersion":1,"accessKeyId":"0123456789abcdef0123456789abcdef","secretAccessKey":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","extra":true}"#
        )
        .is_err());
    }

    #[cfg(target_os = "macos")]
    #[derive(Default)]
    struct FakeKeychainBackend {
        entries: Mutex<HashMap<(String, String), Vec<u8>>>,
        corrupt_reads_for: Mutex<Option<(String, String)>>,
    }

    #[cfg(target_os = "macos")]
    impl FakeKeychainBackend {
        fn entries(&self) -> MutexGuard<'_, HashMap<(String, String), Vec<u8>>> {
            self.entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        fn seed(&self, service: &str, account: &str, payload: &[u8]) {
            self.entries()
                .insert((service.to_owned(), account.to_owned()), payload.to_vec());
        }

        fn contains(&self, service: &str, account: &str) -> bool {
            self.entries()
                .contains_key(&(service.to_owned(), account.to_owned()))
        }

        fn corrupt_reads_for(&self, service: &str, account: &str) {
            *self
                .corrupt_reads_for
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                Some((service.to_owned(), account.to_owned()));
        }
    }

    #[cfg(target_os = "macos")]
    impl KeychainBackend for FakeKeychainBackend {
        fn load(&self, service: &str, account: &str) -> Result<Vec<u8>, CredentialError> {
            let key = (service.to_owned(), account.to_owned());
            let mut payload = self
                .entries()
                .get(&key)
                .cloned()
                .ok_or(CredentialError::Missing)?;
            if self
                .corrupt_reads_for
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                == Some(&key)
            {
                payload.push(0);
            }
            Ok(payload)
        }

        fn save(
            &self,
            service: &str,
            account: &str,
            payload: &[u8],
        ) -> Result<(), CredentialError> {
            self.seed(service, account, payload);
            Ok(())
        }

        fn remove(&self, service: &str, account: &str) -> Result<(), CredentialError> {
            self.entries()
                .remove(&(service.to_owned(), account.to_owned()));
            Ok(())
        }
    }

    #[cfg(target_os = "macos")]
    struct MigrationRaceBackend {
        current_service: String,
        legacy_service: String,
        account: String,
        payload: Vec<u8>,
        migration_completed: Mutex<bool>,
    }

    #[cfg(target_os = "macos")]
    impl KeychainBackend for MigrationRaceBackend {
        fn load(&self, service: &str, account: &str) -> Result<Vec<u8>, CredentialError> {
            if account != self.account {
                return Err(CredentialError::Missing);
            }
            let mut migration_completed = self
                .migration_completed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if service == self.current_service {
                return if *migration_completed {
                    Ok(self.payload.clone())
                } else {
                    Err(CredentialError::Missing)
                };
            }
            if service == self.legacy_service {
                *migration_completed = true;
            }
            Err(CredentialError::Missing)
        }

        fn save(
            &self,
            _service: &str,
            _account: &str,
            _payload: &[u8],
        ) -> Result<(), CredentialError> {
            Err(CredentialError::Unavailable)
        }

        fn remove(&self, _service: &str, _account: &str) -> Result<(), CredentialError> {
            Ok(())
        }
    }
}

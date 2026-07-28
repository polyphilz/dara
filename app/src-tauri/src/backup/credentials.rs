use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::domain::BackupSetId;

const KEYCHAIN_SERVICE: &str = "com.rohan.dara.offsite-backup.r2";
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
impl MacOsKeychainCredentialStore {
    fn entry(backup_set_id: &BackupSetId) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(KEYCHAIN_SERVICE, backup_set_id.as_str())
            .map_err(|_| CredentialError::Unavailable)
    }
}

#[cfg(target_os = "macos")]
impl CredentialStore for MacOsKeychainCredentialStore {
    fn save(
        &self,
        backup_set_id: &BackupSetId,
        credentials: &R2Credentials,
    ) -> Result<(), CredentialError> {
        let payload = credentials.encode()?;
        Self::entry(backup_set_id)?
            .set_secret(&payload)
            .map_err(|_| CredentialError::Unavailable)
    }

    fn load(&self, backup_set_id: &BackupSetId) -> Result<R2Credentials, CredentialError> {
        let mut payload = Self::entry(backup_set_id)?
            .get_secret()
            .map_err(map_keyring_load_error)?;
        let credentials = R2Credentials::decode(&payload);
        payload.zeroize();
        credentials
    }

    fn remove(&self, backup_set_id: &BackupSetId) -> Result<(), CredentialError> {
        match Self::entry(backup_set_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(CredentialError::Unavailable),
        }
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
}

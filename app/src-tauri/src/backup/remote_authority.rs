use super::{
    credentials::CredentialError,
    domain::{BackupErrorCode, IdentityManifestV1, InstallationId, OwnerManifestV1},
    object_store::{ObjectStore, ObjectStoreErrorCode},
};
use crate::database::OffsiteBackupConfig;

pub(crate) fn validate_backup_identity(
    store: &dyn ObjectStore,
    config: &OffsiteBackupConfig,
) -> Result<(), BackupErrorCode> {
    let key = config.target.keyspace().identity();
    match store
        .head(&key)
        .map_err(|error| map_store_error(error.code))?
    {
        Some(_) => {}
        None => return Err(BackupErrorCode::PrefixIdentityMismatch),
    }
    let identity = store
        .get(&key)
        .map_err(|error| map_store_error(error.code))?;
    let identity = IdentityManifestV1::from_json(&identity.bytes)
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    if identity.backup_set_id() != &config.backup_set_id {
        return Err(BackupErrorCode::PrefixIdentityMismatch);
    }
    Ok(())
}

pub(crate) fn validate_backup_authority(
    store: &dyn ObjectStore,
    config: &OffsiteBackupConfig,
    installation_id: &InstallationId,
) -> Result<(), BackupErrorCode> {
    validate_backup_identity(store, config)?;
    let key = config.target.keyspace().owner();
    match store
        .head(&key)
        .map_err(|error| map_store_error(error.code))?
    {
        Some(_) => {}
        None => return Err(BackupErrorCode::OwnerMismatch),
    }
    let owner = store
        .get(&key)
        .map_err(|error| map_store_error(error.code))?;
    let owner =
        OwnerManifestV1::from_json(&owner.bytes).map_err(|_| BackupErrorCode::MalformedManifest)?;
    if !owner.matches(
        &config.backup_set_id,
        installation_id,
        &config.replica_epoch_id,
    ) {
        return Err(BackupErrorCode::OwnerMismatch);
    }
    Ok(())
}

pub(crate) fn map_store_error(error: ObjectStoreErrorCode) -> BackupErrorCode {
    match error {
        ObjectStoreErrorCode::Network => BackupErrorCode::NetworkOffline,
        ObjectStoreErrorCode::Timeout => BackupErrorCode::NetworkTimeout,
        ObjectStoreErrorCode::RateLimited => BackupErrorCode::RateLimited,
        ObjectStoreErrorCode::ServiceUnavailable => BackupErrorCode::ServiceUnavailable,
        ObjectStoreErrorCode::NotFound => BackupErrorCode::RemoteMediaMissing,
        ObjectStoreErrorCode::AuthenticationRejected => BackupErrorCode::AuthenticationRejected,
        ObjectStoreErrorCode::AuthorizationRejected => BackupErrorCode::AuthorizationRejected,
        ObjectStoreErrorCode::InvalidConfiguration | ObjectStoreErrorCode::KeyOutsidePrefix => {
            BackupErrorCode::InvalidTarget
        }
        ObjectStoreErrorCode::ObjectTooLarge => BackupErrorCode::LocalMediaTooLarge,
        ObjectStoreErrorCode::ResponseTooLarge | ObjectStoreErrorCode::InvalidResponse => {
            BackupErrorCode::RemoteMediaCorrupt
        }
        ObjectStoreErrorCode::Conflict | ObjectStoreErrorCode::PreconditionFailed => {
            BackupErrorCode::ImmutableObjectConflict
        }
    }
}

pub(crate) fn map_credential_error(error: CredentialError) -> BackupErrorCode {
    match error {
        CredentialError::Missing => BackupErrorCode::KeychainCredentialMissing,
        CredentialError::Unavailable
        | CredentialError::InvalidCredential(_)
        | CredentialError::UnsupportedPayloadVersion
        | CredentialError::CorruptPayload => BackupErrorCode::KeychainUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{
        domain::{
            BackupProvider, BackupSetId, R2AccountId, R2BucketName, R2Jurisdiction, R2Prefix,
            R2Target, ReplicaEpochId, UtcTimestamp,
        },
        object_store::{
            fake::FakeObjectStore, ObjectContentType, PutCondition, PutObjectOutcome,
            PutObjectRequest,
        },
    };

    fn config() -> OffsiteBackupConfig {
        OffsiteBackupConfig {
            revision: 1,
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            enabled: true,
            provider: BackupProvider::R2,
            target: R2Target {
                account_id: R2AccountId::parse("0123456789abcdef0123456789abcdef")
                    .expect("account ID"),
                jurisdiction: R2Jurisdiction::Default,
                bucket: R2BucketName::parse("dara-test").expect("bucket"),
                prefix: R2Prefix::parse("dara/authority-test").expect("prefix"),
            },
            created_at: 1,
            updated_at: 1,
        }
    }

    fn put_json(store: &FakeObjectStore, key: crate::backup::domain::R2ObjectKey, bytes: Vec<u8>) {
        assert_eq!(
            store
                .put(PutObjectRequest {
                    key,
                    bytes,
                    content_type: ObjectContentType::Json,
                    dara_sha256: None,
                    condition: PutCondition::IfAbsent,
                })
                .expect("manifest write"),
            PutObjectOutcome::Stored
        );
    }

    fn put_identity_and_owner(
        store: &FakeObjectStore,
        config: &OffsiteBackupConfig,
        installation_id: InstallationId,
    ) {
        let keyspace = config.target.keyspace();
        put_json(
            store,
            keyspace.identity(),
            IdentityManifestV1::new(config.backup_set_id.clone(), installation_id.clone())
                .to_json()
                .expect("identity JSON"),
        );
        put_json(
            store,
            keyspace.owner(),
            OwnerManifestV1::new(
                config.backup_set_id.clone(),
                installation_id,
                config.replica_epoch_id.clone(),
                UtcTimestamp::parse("2026-07-27T16:28:33Z").expect("timestamp"),
            )
            .to_json()
            .expect("owner JSON"),
        );
    }

    #[test]
    fn authority_requires_matching_identity_owner_installation_and_epoch() {
        let store = FakeObjectStore::default();
        let config = config();
        let installation_id = InstallationId::new();
        put_identity_and_owner(&store, &config, installation_id.clone());

        assert_eq!(
            validate_backup_authority(&store, &config, &installation_id),
            Ok(())
        );
        assert_eq!(
            validate_backup_authority(&store, &config, &InstallationId::new()),
            Err(BackupErrorCode::OwnerMismatch)
        );
    }

    #[test]
    fn missing_or_malformed_authority_never_allows_replication() {
        let store = FakeObjectStore::default();
        let config = config();
        assert_eq!(
            validate_backup_authority(&store, &config, &InstallationId::new()),
            Err(BackupErrorCode::PrefixIdentityMismatch)
        );

        put_json(
            &store,
            config.target.keyspace().identity(),
            b"{\"not\":\"an identity manifest\"}".to_vec(),
        );
        assert_eq!(
            validate_backup_authority(&store, &config, &InstallationId::new()),
            Err(BackupErrorCode::MalformedManifest)
        );
    }
}

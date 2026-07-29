use std::time::{Duration, Instant};

use super::{
    credentials::CredentialError,
    domain::{
        BackupErrorCode, IdentityManifestV1, InstallationId, OwnerManifestV1, ReplicaEpochId,
        UtcTimestamp,
    },
    object_store::{
        ObjectContentType, ObjectStore, ObjectStoreErrorCode, PutCondition, PutObjectOutcome,
        PutObjectRequest,
    },
};
use crate::database::OffsiteBackupConfig;

pub(crate) fn create_or_validate_backup_authority(
    store: &dyn ObjectStore,
    config: &OffsiteBackupConfig,
    installation_id: &InstallationId,
) -> Result<(), BackupErrorCode> {
    let keyspace = config.target.keyspace();
    let identity = IdentityManifestV1::new(config.backup_set_id.clone(), installation_id.clone())
        .to_json()
        .map_err(|_| BackupErrorCode::InvalidTarget)?;
    let identity_outcome = store
        .put(PutObjectRequest {
            key: keyspace.identity(),
            bytes: identity,
            content_type: ObjectContentType::Json,
            dara_sha256: None,
            condition: PutCondition::IfAbsent,
        })
        .map_err(|error| map_store_error(error.code))?;
    if identity_outcome == PutObjectOutcome::ConditionNotMet {
        validate_backup_identity(store, config)?;
    }

    let owner = OwnerManifestV1::new(
        config.backup_set_id.clone(),
        installation_id.clone(),
        config.replica_epoch_id.clone(),
        UtcTimestamp::now().map_err(|_| BackupErrorCode::WorkerUnavailable)?,
    )
    .to_json()
    .map_err(|_| BackupErrorCode::InvalidTarget)?;
    let owner_outcome = store
        .put(PutObjectRequest {
            key: keyspace.owner(),
            bytes: owner,
            content_type: ObjectContentType::Json,
            dara_sha256: None,
            condition: PutCondition::IfAbsent,
        })
        .map_err(|error| map_store_error(error.code))?;
    if owner_outcome == PutObjectOutcome::ConditionNotMet {
        validate_backup_authority(store, config, installation_id)?;
    }
    validate_backup_authority(store, config, installation_id)
}

pub(crate) fn take_over_backup_authority(
    store: &dyn ObjectStore,
    config: &OffsiteBackupConfig,
    installation_id: &InstallationId,
    replica_epoch_id: &ReplicaEpochId,
) -> Result<ReplicaEpochId, BackupErrorCode> {
    validate_backup_identity(store, config)?;
    let owner_key = config.target.keyspace().owner();
    let current = store
        .get(&owner_key)
        .map_err(|error| map_store_error(error.code))?;
    let current_owner = OwnerManifestV1::from_json(&current.bytes)
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    if current_owner.backup_set_id() != &config.backup_set_id {
        return Err(BackupErrorCode::PrefixIdentityMismatch);
    }
    if current_owner.installation_id() == installation_id
        && current_owner.replica_epoch_id() != &config.replica_epoch_id
    {
        let adopted_epoch = current_owner.replica_epoch_id().clone();
        let candidate = OffsiteBackupConfig {
            replica_epoch_id: adopted_epoch.clone(),
            ..config.clone()
        };
        validate_backup_authority(store, &candidate, installation_id)?;
        return Ok(adopted_epoch);
    }
    let replacement = OwnerManifestV1::new(
        config.backup_set_id.clone(),
        installation_id.clone(),
        replica_epoch_id.clone(),
        UtcTimestamp::now().map_err(|_| BackupErrorCode::WorkerUnavailable)?,
    )
    .to_json()
    .map_err(|_| BackupErrorCode::InvalidTarget)?;
    let outcome = store
        .put(PutObjectRequest {
            key: owner_key,
            bytes: replacement,
            content_type: ObjectContentType::Json,
            dara_sha256: None,
            condition: PutCondition::IfMatch(current.metadata.version),
        })
        .map_err(|error| map_store_error(error.code))?;
    if outcome != PutObjectOutcome::Stored {
        return Err(BackupErrorCode::OwnerMismatch);
    }
    let candidate = OffsiteBackupConfig {
        replica_epoch_id: replica_epoch_id.clone(),
        ..config.clone()
    };
    validate_backup_authority(store, &candidate, installation_id)?;
    Ok(replica_epoch_id.clone())
}

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

pub(crate) fn validate_backup_authority_with_deadline(
    store: &dyn ObjectStore,
    config: &OffsiteBackupConfig,
    installation_id: &InstallationId,
    deadline: Instant,
) -> Result<(), BackupErrorCode> {
    let identity_key = config.target.keyspace().identity();
    match store
        .head_with_timeout(&identity_key, remaining(deadline)?)
        .map_err(|error| map_store_error(error.code))?
    {
        Some(_) => {}
        None => return Err(BackupErrorCode::PrefixIdentityMismatch),
    }
    let identity = store
        .get_with_timeout(&identity_key, remaining(deadline)?)
        .map_err(|error| map_store_error(error.code))?;
    let identity = IdentityManifestV1::from_json(&identity.bytes)
        .map_err(|_| BackupErrorCode::MalformedManifest)?;
    if identity.backup_set_id() != &config.backup_set_id {
        return Err(BackupErrorCode::PrefixIdentityMismatch);
    }

    let owner_key = config.target.keyspace().owner();
    match store
        .head_with_timeout(&owner_key, remaining(deadline)?)
        .map_err(|error| map_store_error(error.code))?
    {
        Some(_) => {}
        None => return Err(BackupErrorCode::OwnerMismatch),
    }
    let owner = store
        .get_with_timeout(&owner_key, remaining(deadline)?)
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

fn remaining(deadline: Instant) -> Result<Duration, BackupErrorCode> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(BackupErrorCode::NetworkTimeout)
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

    #[test]
    fn initial_authority_claim_is_conditional_and_idempotent() {
        let store = FakeObjectStore::default();
        let config = config();
        let installation_id = InstallationId::new();

        create_or_validate_backup_authority(&store, &config, &installation_id)
            .expect("initial authority claim");
        create_or_validate_backup_authority(&store, &config, &installation_id)
            .expect("idempotent authority validation");

        validate_backup_authority(&store, &config, &installation_id)
            .expect("claimed authority remains valid");
    }

    #[test]
    fn authority_claim_never_redirects_an_occupied_prefix() {
        let store = FakeObjectStore::default();
        let original = config();
        let original_installation = InstallationId::new();
        create_or_validate_backup_authority(&store, &original, &original_installation)
            .expect("original authority");
        let candidate = OffsiteBackupConfig {
            backup_set_id: BackupSetId::new(),
            replica_epoch_id: ReplicaEpochId::new(),
            ..original.clone()
        };

        assert_eq!(
            create_or_validate_backup_authority(&store, &candidate, &InstallationId::new()),
            Err(BackupErrorCode::PrefixIdentityMismatch)
        );
        validate_backup_authority(&store, &original, &original_installation)
            .expect("original authority was not replaced");
    }

    #[test]
    fn explicit_takeover_rotates_the_epoch_and_fences_the_old_owner() {
        let store = FakeObjectStore::default();
        let original = config();
        let old_installation = InstallationId::new();
        create_or_validate_backup_authority(&store, &original, &old_installation)
            .expect("original authority");
        let new_installation = InstallationId::new();
        let new_epoch = ReplicaEpochId::new();

        let installed_epoch =
            take_over_backup_authority(&store, &original, &new_installation, &new_epoch)
                .expect("takeover");
        assert_eq!(installed_epoch, new_epoch);

        let taken_over = OffsiteBackupConfig {
            replica_epoch_id: new_epoch,
            ..original.clone()
        };
        validate_backup_authority(&store, &taken_over, &new_installation).expect("new owner");
        assert_eq!(
            validate_backup_authority(&store, &original, &old_installation),
            Err(BackupErrorCode::OwnerMismatch)
        );
    }

    #[test]
    fn takeover_retry_adopts_the_epoch_already_claimed_by_this_installation() {
        let store = FakeObjectStore::default();
        let original = config();
        create_or_validate_backup_authority(&store, &original, &InstallationId::new())
            .expect("original authority");
        let new_installation = InstallationId::new();
        let claimed_epoch = ReplicaEpochId::new();

        take_over_backup_authority(&store, &original, &new_installation, &claimed_epoch)
            .expect("initial remote takeover");

        let retry_epoch = ReplicaEpochId::new();
        let adopted_epoch =
            take_over_backup_authority(&store, &original, &new_installation, &retry_epoch)
                .expect("idempotent takeover retry");
        assert_eq!(adopted_epoch, claimed_epoch);
        assert_ne!(adopted_epoch, retry_epoch);

        let recovered = OffsiteBackupConfig {
            replica_epoch_id: adopted_epoch,
            ..original
        };
        validate_backup_authority(&store, &recovered, &new_installation)
            .expect("remote owner remains the recoverable authority");
    }
}

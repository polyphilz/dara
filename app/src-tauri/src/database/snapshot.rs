use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    connection::{self, DatabaseKind, FileState},
    error::{DatabaseError, Result},
    migrations::{self, MigrationHeads},
    paths::DatabasePaths,
    validation,
};

static SNAPSHOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const MANAGED_SNAPSHOT_PREFIX: &str = "snapshot";
const MANAGED_SNAPSHOT_STEM_PREFIX: &str = "snapshot-";
const MIGRATION_SAFETY_SNAPSHOT_PREFIX: &str = "migration-safety";
const MIGRATION_SAFETY_SNAPSHOT_STEM_PREFIX: &str = "migration-safety-";
const RESTORE_SAFETY_SNAPSHOT_PREFIX: &str = "restore-safety";
const RESTORE_SAFETY_SNAPSHOT_STEM_PREFIX: &str = "restore-safety-";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotRetention {
    Managed,
    MigrationSafety,
    RestoreSafety,
}

impl SnapshotRetention {
    const fn file_prefix(self) -> &'static str {
        match self {
            Self::Managed => MANAGED_SNAPSHOT_PREFIX,
            Self::MigrationSafety => MIGRATION_SAFETY_SNAPSHOT_PREFIX,
            Self::RestoreSafety => RESTORE_SAFETY_SNAPSHOT_PREFIX,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnapshotFile {
    pub file_name: String,
    pub sha256: String,
    pub migration_head: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SnapshotManifest {
    pub format_version: u32,
    pub created_at: i64,
    pub application_version: String,
    pub main: SnapshotFile,
    pub media: SnapshotFile,
    pub relationship_validated: bool,
}

#[derive(Clone, Debug)]
pub struct CreatedSnapshot {
    pub manifest_path: PathBuf,
    pub manifest: SnapshotManifest,
}

#[derive(Clone, Debug)]
pub(crate) struct ValidatedSnapshot {
    pub manifest_path: PathBuf,
    pub manifest: SnapshotManifest,
    pub main_path: PathBuf,
    pub media_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizedSnapshotSummary {
    pub created_at: i64,
    pub application_version: String,
}

pub fn create_snapshot_pair(
    paths: &DatabasePaths,
    application_version: &str,
) -> Result<CreatedSnapshot> {
    create_snapshot_pair_with_retention(paths, application_version, SnapshotRetention::Managed)
}

pub(crate) fn create_migration_safety_snapshot_pair(
    paths: &DatabasePaths,
    application_version: &str,
) -> Result<CreatedSnapshot> {
    let snapshot = create_snapshot_pair_with_retention(
        paths,
        application_version,
        SnapshotRetention::MigrationSafety,
    )?;
    remove_redundant_migration_safety_snapshots(&snapshot)?;
    Ok(snapshot)
}

pub(crate) fn create_restore_safety_snapshot_pair(
    paths: &DatabasePaths,
    application_version: &str,
) -> Result<CreatedSnapshot> {
    create_snapshot_pair_with_retention(
        paths,
        application_version,
        SnapshotRetention::RestoreSafety,
    )
}

pub(crate) fn protect_legacy_restore_safety_snapshot(
    backups: &Path,
    manifest_path: &Path,
) -> Result<PathBuf> {
    if manifest_path.parent() != Some(backups) {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "restore safety snapshot must be inside {}",
            backups.display()
        )));
    }
    if is_restore_safety_snapshot(manifest_path) {
        return Ok(manifest_path.to_owned());
    }

    let legacy_identifier = manifest_path
        .file_stem()
        .and_then(|value| value.to_str())
        .and_then(|stem| stem.strip_prefix(MANAGED_SNAPSHOT_STEM_PREFIX))
        .filter(|identifier| !identifier.is_empty())
        .filter(|_| manifest_path.extension().and_then(|value| value.to_str()) == Some("json"))
        .ok_or_else(|| {
            DatabaseError::InvalidSnapshot(format!(
                "legacy restore safety manifest has an unexpected name: {}",
                manifest_path.display()
            ))
        })?;
    let protected_path = backups.join(format!(
        "{RESTORE_SAFETY_SNAPSHOT_STEM_PREFIX}legacy-{legacy_identifier}.json"
    ));

    match (manifest_path.exists(), protected_path.exists()) {
        (true, false) => {
            load_and_validate_snapshot(manifest_path)?;
            fs::rename(manifest_path, &protected_path)?;
            sync_directory(backups)?;
        }
        (false, true) => {
            load_and_validate_snapshot(&protected_path)?;
        }
        (true, true) => {
            return Err(DatabaseError::InvalidSnapshot(format!(
                "legacy and protected restore safety manifests both exist: {} and {}",
                manifest_path.display(),
                protected_path.display()
            )));
        }
        (false, false) => {
            return Err(DatabaseError::InvalidSnapshot(format!(
                "restore safety manifest does not exist: {}",
                manifest_path.display()
            )));
        }
    }

    Ok(protected_path)
}

pub(crate) fn remove_created_snapshot_pair(snapshot: &CreatedSnapshot) -> Result<()> {
    let directory = snapshot.manifest_path.parent().ok_or_else(|| {
        DatabaseError::InvalidSnapshot("manifest has no containing directory".into())
    })?;
    for file_name in [
        &snapshot.manifest.main.file_name,
        &snapshot.manifest.media.file_name,
    ] {
        remove_if_exists(&resolve_snapshot_file(directory, file_name)?)?;
    }
    remove_if_exists(&snapshot.manifest_path)?;
    sync_directory(directory)
}

fn create_snapshot_pair_with_retention(
    paths: &DatabasePaths,
    application_version: &str,
    retention: SnapshotRetention,
) -> Result<CreatedSnapshot> {
    let mut main_source =
        connection::open_writer(&paths.main, DatabaseKind::Main, FileState::Existing)?;
    let mut media_source =
        connection::open_writer(&paths.media, DatabaseKind::Media, FileState::Existing)?;
    create_snapshot_pair_from_connections_with_retention(
        paths,
        application_version,
        &mut main_source,
        &mut media_source,
        retention,
    )
}

pub(super) fn create_snapshot_pair_from_connections(
    paths: &DatabasePaths,
    application_version: &str,
    main_source: &mut Connection,
    media_source: &mut Connection,
) -> Result<CreatedSnapshot> {
    create_snapshot_pair_from_connections_with_retention(
        paths,
        application_version,
        main_source,
        media_source,
        SnapshotRetention::Managed,
    )
}

pub(crate) fn finalize_external_snapshot_pair(
    backups: &Path,
    main_path: &Path,
    media_path: &Path,
    created_at: i64,
    application_version: &str,
    recorded_heads: MigrationHeads,
) -> Result<CreatedSnapshot> {
    timestamp(created_at)?;
    if application_version.is_empty()
        || application_version.len() > 64
        || application_version.chars().any(char::is_control)
    {
        return Err(DatabaseError::InvalidSnapshot(
            "application version is invalid".into(),
        ));
    }
    if main_path.parent() != Some(backups) || media_path.parent() != Some(backups) {
        return Err(DatabaseError::InvalidSnapshot(
            "external snapshot files must be direct children of the backups directory".into(),
        ));
    }
    fs::create_dir_all(backups)?;
    sync_file(main_path)?;
    sync_file(media_path)?;

    let mut main = connection::open_read_only(main_path, DatabaseKind::Main)?;
    let mut media = connection::open_read_only(media_path, DatabaseKind::Media)?;
    let relationship_validated =
        validation::validate_snapshot_pair(&mut main, &mut media, main_path, media_path)?;
    let actual_heads = migrations::current_heads(&mut main, &mut media)?;
    if actual_heads != recorded_heads {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "migration heads are {actual_heads:?}, expected {recorded_heads:?}"
        )));
    }
    drop(main);
    drop(media);

    let identifier = Uuid::now_v7();
    let manifest_name = format!("remote-restore-{identifier}.json");
    let manifest_temp = backups.join(format!(".{manifest_name}.tmp"));
    let manifest_final = backups.join(manifest_name);
    let manifest = SnapshotManifest {
        format_version: 1,
        created_at,
        application_version: application_version.to_owned(),
        main: SnapshotFile {
            file_name: snapshot_file_name(main_path)?,
            sha256: hash_file(main_path)?,
            migration_head: recorded_heads.main,
        },
        media: SnapshotFile {
            file_name: snapshot_file_name(media_path)?,
            sha256: hash_file(media_path)?,
            migration_head: recorded_heads.media,
        },
        relationship_validated,
    };
    write_manifest(&manifest_temp, &manifest)?;
    fs::rename(&manifest_temp, &manifest_final)?;
    sync_directory(backups)?;
    Ok(CreatedSnapshot {
        manifest_path: manifest_final,
        manifest,
    })
}

fn create_snapshot_pair_from_connections_with_retention(
    paths: &DatabasePaths,
    application_version: &str,
    main_source: &mut Connection,
    media_source: &mut Connection,
    retention: SnapshotRetention,
) -> Result<CreatedSnapshot> {
    fs::create_dir_all(&paths.backups)?;

    let created_at = now_millis()?;
    let sequence = SNAPSHOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stem = format!(
        "{}-{created_at}-{}-{sequence}",
        retention.file_prefix(),
        std::process::id()
    );
    let main_name = format!("{stem}-main.sqlite3");
    let media_name = format!("{stem}-media.sqlite3");
    let manifest_name = format!("{stem}.json");

    let main_temp = paths.backups.join(format!(".{main_name}.tmp"));
    let media_temp = paths.backups.join(format!(".{media_name}.tmp"));
    let manifest_temp = paths.backups.join(format!(".{manifest_name}.tmp"));
    let main_final = paths.backups.join(&main_name);
    let media_final = paths.backups.join(&media_name);
    let manifest_final = paths.backups.join(&manifest_name);

    let heads = migrations::current_heads(main_source, media_source)?;

    vacuum_into(main_source, &main_temp)?;
    vacuum_into(media_source, &media_temp)?;
    sync_file(&main_temp)?;
    sync_file(&media_temp)?;

    let mut main_snapshot = connection::open_read_only(&main_temp, DatabaseKind::Main)?;
    let mut media_snapshot = connection::open_read_only(&media_temp, DatabaseKind::Media)?;
    let relationship_validated = validation::validate_snapshot_pair(
        &mut main_snapshot,
        &mut media_snapshot,
        &main_temp,
        &media_temp,
    )?;
    drop(main_snapshot);
    drop(media_snapshot);

    let main_hash = hash_file(&main_temp)?;
    let media_hash = hash_file(&media_temp)?;

    fs::rename(&main_temp, &main_final)?;
    fs::rename(&media_temp, &media_final)?;
    sync_directory(&paths.backups)?;

    let manifest = SnapshotManifest {
        format_version: 1,
        created_at,
        application_version: application_version.to_owned(),
        main: SnapshotFile {
            file_name: main_name,
            sha256: main_hash,
            migration_head: heads.main,
        },
        media: SnapshotFile {
            file_name: media_name,
            sha256: media_hash,
            migration_head: heads.media,
        },
        relationship_validated,
    };

    write_manifest(&manifest_temp, &manifest)?;
    fs::rename(&manifest_temp, &manifest_final)?;
    sync_directory(&paths.backups)?;

    Ok(CreatedSnapshot {
        manifest_path: manifest_final,
        manifest,
    })
}

pub fn load_and_validate_manifest(path: &Path) -> Result<SnapshotManifest> {
    Ok(load_and_validate_snapshot(path)?.manifest)
}

pub(crate) fn load_and_validate_snapshot(path: &Path) -> Result<ValidatedSnapshot> {
    let file = File::open(path)?;
    let manifest: SnapshotManifest = serde_json::from_reader(BufReader::new(file))?;
    if manifest.format_version != 1 {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "unsupported format version {}",
            manifest.format_version
        )));
    }
    if !manifest.relationship_validated {
        return Err(DatabaseError::InvalidSnapshot(
            "manifest was not finalized after relationship validation".into(),
        ));
    }
    timestamp(manifest.created_at)?;

    let directory = path.parent().ok_or_else(|| {
        DatabaseError::InvalidSnapshot("manifest has no containing directory".into())
    })?;
    let main_path = resolve_snapshot_file(directory, &manifest.main.file_name)?;
    let media_path = resolve_snapshot_file(directory, &manifest.media.file_name)?;
    validate_snapshot_pair_files(&manifest, &main_path, &media_path)?;
    Ok(ValidatedSnapshot {
        manifest_path: path.to_owned(),
        manifest,
        main_path,
        media_path,
    })
}

pub(crate) fn validate_snapshot_pair_files(
    manifest: &SnapshotManifest,
    main_path: &Path,
    media_path: &Path,
) -> Result<()> {
    validate_recorded_hash(main_path, &manifest.main.sha256)?;
    validate_recorded_hash(media_path, &manifest.media.sha256)?;
    let mut main = connection::open_read_only(main_path, DatabaseKind::Main)?;
    let mut media = connection::open_read_only(media_path, DatabaseKind::Media)?;
    validation::validate_snapshot_pair(&mut main, &mut media, main_path, media_path)?;
    let heads = migrations::current_heads(&mut main, &mut media)?;
    let recorded = MigrationHeads {
        main: manifest.main.migration_head,
        media: manifest.media.migration_head,
    };
    if heads != recorded {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "migration heads are {heads:?}, manifest records {recorded:?}"
        )));
    }
    Ok(())
}

pub fn restore_snapshot_pair(manifest_path: &Path, target: &DatabasePaths) -> Result<()> {
    let manifest = load_and_validate_manifest(manifest_path)?;
    if target.main.exists() || target.media.exists() {
        return Err(DatabaseError::InvalidSnapshot(
            "restore target must not contain an existing database pair".into(),
        ));
    }
    fs::create_dir_all(target.root())?;

    let directory = manifest_path.parent().ok_or_else(|| {
        DatabaseError::InvalidSnapshot("manifest has no containing directory".into())
    })?;
    let source_main = resolve_snapshot_file(directory, &manifest.main.file_name)?;
    let source_media = resolve_snapshot_file(directory, &manifest.media.file_name)?;
    let temp_main = target.root.join(".dara.sqlite3.restore.tmp");
    let temp_media = target.root.join(".media.sqlite3.restore.tmp");

    copy_and_sync(&source_main, &temp_main)?;
    copy_and_sync(&source_media, &temp_media)?;
    let mut main = connection::open_read_only(&temp_main, DatabaseKind::Main)?;
    let mut media = connection::open_read_only(&temp_media, DatabaseKind::Media)?;
    validation::validate_snapshot_pair(&mut main, &mut media, &temp_main, &temp_media)?;
    drop(main);
    drop(media);

    fs::rename(&temp_media, &target.media)?;
    fs::rename(&temp_main, &target.main)?;
    sync_directory(target.root())?;
    Ok(())
}

pub fn prune_snapshots(backups: &Path) -> Result<()> {
    if !backups.exists() {
        return Ok(());
    }

    let mut snapshots = Vec::new();
    for entry in fs::read_dir(backups)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let manifest: SnapshotManifest =
            match serde_json::from_reader::<_, SnapshotManifest>(BufReader::new(file)) {
                Ok(manifest)
                    if manifest.format_version == 1 && timestamp(manifest.created_at).is_ok() =>
                {
                    manifest
                }
                _ => continue,
            };
        snapshots.push((path, manifest));
    }
    snapshots.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.1.created_at));

    let mut keep = HashSet::new();
    let mut daily = HashSet::new();
    let mut weekly = HashSet::new();
    let mut monthly = HashSet::new();
    let mut migration_safety_heads = HashSet::new();
    for (path, manifest) in &snapshots {
        if is_restore_safety_snapshot(path) {
            keep.insert(path.clone());
            continue;
        }
        if is_migration_safety_snapshot(path) && manifest.relationship_validated {
            let heads = (manifest.main.migration_head, manifest.media.migration_head);
            if migration_safety_heads.insert(heads) {
                keep.insert(path.clone());
            }
            continue;
        }
        let datetime = timestamp(manifest.created_at)?;
        let date = datetime.date();
        let day_key = (date.year(), date.ordinal());
        let (iso_year, iso_week, _) = date.to_iso_week_date();
        let week_key = (iso_year, iso_week);
        let month_key = (date.year(), u8::from(date.month()));

        let keep_daily = daily.len() < 7 && daily.insert(day_key);
        let keep_weekly = weekly.len() < 4 && weekly.insert(week_key);
        let keep_monthly = monthly.len() < 6 && monthly.insert(month_key);
        if keep_daily || keep_weekly || keep_monthly {
            keep.insert(path.clone());
        }
    }

    for (manifest_path, manifest) in snapshots {
        if keep.contains(&manifest_path) {
            continue;
        }
        let directory = manifest_path.parent().ok_or_else(|| {
            DatabaseError::InvalidSnapshot("manifest has no containing directory".into())
        })?;
        for file_name in [&manifest.main.file_name, &manifest.media.file_name] {
            if let Ok(path) = resolve_snapshot_file(directory, file_name) {
                remove_if_exists(&path)?;
            }
        }
        remove_if_exists(&manifest_path)?;
    }
    sync_directory(backups)?;
    Ok(())
}

fn is_restore_safety_snapshot(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|stem| stem.starts_with(RESTORE_SAFETY_SNAPSHOT_STEM_PREFIX))
}

fn is_migration_safety_snapshot(path: &Path) -> bool {
    path.file_stem()
        .and_then(|value| value.to_str())
        .is_some_and(|stem| stem.starts_with(MIGRATION_SAFETY_SNAPSHOT_STEM_PREFIX))
}

fn remove_redundant_migration_safety_snapshots(snapshot: &CreatedSnapshot) -> Result<()> {
    let backups = snapshot.manifest_path.parent().ok_or_else(|| {
        DatabaseError::InvalidSnapshot("migration safety manifest has no directory".into())
    })?;
    let heads = MigrationHeads {
        main: snapshot.manifest.main.migration_head,
        media: snapshot.manifest.media.migration_head,
    };
    for entry in fs::read_dir(backups)? {
        let path = entry?.path();
        if path == snapshot.manifest_path
            || !is_migration_safety_snapshot(&path)
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        let validated = match load_and_validate_snapshot(&path) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        let candidate_heads = MigrationHeads {
            main: validated.manifest.main.migration_head,
            media: validated.manifest.media.migration_head,
        };
        if candidate_heads != heads {
            continue;
        }
        remove_created_snapshot_pair(&CreatedSnapshot {
            manifest_path: validated.manifest_path,
            manifest: validated.manifest,
        })?;
    }
    Ok(())
}

pub(crate) fn latest_finalized_snapshot(
    backups: &Path,
) -> Result<Option<FinalizedSnapshotSummary>> {
    if !backups.exists() {
        return Ok(None);
    }

    let mut latest: Option<FinalizedSnapshotSummary> = None;
    for entry in fs::read_dir(backups)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let manifest = match serde_json::from_reader::<_, SnapshotManifest>(BufReader::new(file)) {
            Ok(manifest)
                if manifest.format_version == 1
                    && manifest.relationship_validated
                    && timestamp(manifest.created_at).is_ok() =>
            {
                manifest
            }
            _ => continue,
        };
        let directory = match path.parent() {
            Some(directory) => directory,
            None => continue,
        };
        let main_path = match resolve_snapshot_file(directory, &manifest.main.file_name) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let media_path = match resolve_snapshot_file(directory, &manifest.media.file_name) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !main_path.is_file() || !media_path.is_file() {
            continue;
        }
        let candidate = FinalizedSnapshotSummary {
            created_at: manifest.created_at,
            application_version: manifest.application_version,
        };
        if latest
            .as_ref()
            .is_none_or(|current| candidate.created_at > current.created_at)
        {
            latest = Some(candidate);
        }
    }
    Ok(latest)
}

fn vacuum_into(connection: &Connection, destination: &Path) -> Result<()> {
    remove_if_exists(destination)?;
    let destination = destination
        .to_str()
        .ok_or_else(|| DatabaseError::InvalidSnapshot("snapshot path is not valid UTF-8".into()))?;
    connection.execute("VACUUM INTO ?1", [destination])?;
    Ok(())
}

fn write_manifest(path: &Path, manifest: &SnapshotManifest) -> Result<()> {
    let file = OpenOptions::new().create_new(true).write(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, manifest)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn copy_and_sync(source: &Path, destination: &Path) -> Result<()> {
    remove_if_exists(destination)?;
    fs::copy(source, destination)?;
    sync_file(destination)
}

fn sync_file(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn validate_recorded_hash(path: &Path, expected: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "{} is not a regular snapshot file",
            path.display()
        )));
    }
    let actual = hash_file(path)?;
    if actual != expected {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "{} has digest {actual}, expected {expected}",
            path.display()
        )));
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn resolve_snapshot_file(directory: &Path, file_name: &str) -> Result<PathBuf> {
    let mut components = Path::new(file_name).components();
    let valid =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !valid {
        return Err(DatabaseError::InvalidSnapshot(format!(
            "unsafe snapshot file name {file_name:?}"
        )));
    }
    Ok(directory.join(file_name))
}

fn snapshot_file_name(path: &Path) -> Result<String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            DatabaseError::InvalidSnapshot("snapshot file name is not valid UTF-8".into())
        })?;
    resolve_snapshot_file(
        path.parent().ok_or_else(|| {
            DatabaseError::InvalidSnapshot("snapshot file has no containing directory".into())
        })?,
        file_name,
    )?;
    Ok(file_name.to_owned())
}

fn timestamp(milliseconds: i64) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(milliseconds) * 1_000_000)
        .map_err(|error| DatabaseError::InvalidSnapshot(error.to_string()))
}

fn now_millis() -> Result<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| DatabaseError::InvalidSystemTime)?;
    i64::try_from(duration.as_millis()).map_err(|_| DatabaseError::InvalidSystemTime)
}

use std::{fs, path::Path, sync::OnceLock, time::Duration};

use rusqlite::{Connection, OpenFlags};

use super::error::{DatabaseError, Result};

pub const MAIN_APPLICATION_ID: i32 = i32::from_be_bytes(*b"DARA");
pub const MEDIA_APPLICATION_ID: i32 = i32::from_be_bytes(*b"DMED");

static SQLITE_VEC_REGISTRATION: OnceLock<std::result::Result<(), i32>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseKind {
    Main,
    Media,
}

impl DatabaseKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Media => "media",
        }
    }

    pub const fn application_id(self) -> i32 {
        match self {
            Self::Main => MAIN_APPLICATION_ID,
            Self::Media => MEDIA_APPLICATION_ID,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileState {
    Fresh,
    Existing,
}

impl FileState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Existing => "existing",
        }
    }
}

pub fn register_sqlite_vec() -> Result<()> {
    let result = SQLITE_VEC_REGISTRATION.get_or_init(|| {
        type ExtensionEntry = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::ffi::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::ffi::c_int;

        // sqlite-vec exposes a C SQLite extension entry point. SQLite requires
        // the erased auto-extension callback signature used by its C API.
        let code = unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                ExtensionEntry,
            >(
                sqlite_vec::sqlite3_vec_init as *const ()
            )))
        };
        if code == rusqlite::ffi::SQLITE_OK {
            Ok(())
        } else {
            Err(code)
        }
    });

    result.map_err(DatabaseError::VecRegistration)
}

pub fn inspect_file(path: &Path) -> Result<FileState> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() > 0 => Ok(FileState::Existing),
        Ok(_) => Ok(FileState::Fresh),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(FileState::Fresh),
        Err(error) => Err(error.into()),
    }
}

pub fn open_writer(path: &Path, kind: DatabaseKind, state: FileState) -> Result<Connection> {
    register_sqlite_vec()?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)?;
    if state == FileState::Existing {
        verify_application_id(&connection, path, kind)?;
    }
    configure_writer(&connection)?;
    verify_or_set_application_id(&connection, path, kind, state)?;
    Ok(connection)
}

pub fn open_read_only(path: &Path, kind: DatabaseKind) -> Result<Connection> {
    register_sqlite_vec()?;
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)?;
    connection.busy_timeout(Duration::from_millis(5_000))?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "query_only", "ON")?;
    verify_application_id(&connection, path, kind)?;
    Ok(connection)
}

fn configure_writer(connection: &Connection) -> Result<()> {
    connection.busy_timeout(Duration::from_millis(5_000))?;

    let journal_mode: String =
        connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(DatabaseError::Validation {
            kind: "connection",
            reason: format!("journal_mode is {journal_mode}, not WAL"),
        });
    }

    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "cache_size", -20_000_i64)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;

    verify_pragma_i64(connection, "synchronous", 2)?;
    verify_pragma_i64(connection, "foreign_keys", 1)?;
    verify_pragma_i64(connection, "trusted_schema", 0)?;
    verify_pragma_i64(connection, "cache_size", -20_000)?;
    verify_pragma_i64(connection, "temp_store", 2)?;
    Ok(())
}

fn verify_pragma_i64(connection: &Connection, pragma: &'static str, expected: i64) -> Result<()> {
    let actual: i64 = connection.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
    if actual != expected {
        return Err(DatabaseError::Validation {
            kind: "connection",
            reason: format!("PRAGMA {pragma} is {actual}, expected {expected}"),
        });
    }
    Ok(())
}

fn verify_or_set_application_id(
    connection: &Connection,
    path: &Path,
    kind: DatabaseKind,
    state: FileState,
) -> Result<()> {
    if state == FileState::Fresh {
        connection.pragma_update(None, "application_id", kind.application_id())?;
    }
    verify_application_id(connection, path, kind)
}

pub fn verify_application_id(
    connection: &Connection,
    path: &Path,
    kind: DatabaseKind,
) -> Result<()> {
    let actual: i32 = connection.query_row("PRAGMA application_id", [], |row| row.get(0))?;
    let expected = kind.application_id();
    if actual != expected {
        return Err(DatabaseError::WrongApplicationId {
            kind: kind.label(),
            path: path.to_path_buf(),
            expected,
            actual,
        });
    }
    Ok(())
}

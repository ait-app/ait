//! Opening single-writer SQLite databases in the daemon data directory.

use std::fs;
use std::io;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

/// Open the database file at `database` for one writer, creating it and its directory with
/// private permissions when absent.
/// # Errors
/// Returns the caller's error for a path that cannot be created or a database that cannot be
/// opened.
pub(super) fn open<E>(database: &Path) -> Result<Connection, E>
where
    E: From<io::Error> + From<rusqlite::Error>,
{
    let parent = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    let database = parent.join(
        database
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?,
    );
    if !database.try_exists()? {
        // SQLite inherits the permissions of the precreated database for its journal.
        let temporary = tempfile::NamedTempFile::new_in(parent)?;
        if let Err(error) = temporary.persist_noclobber(&database)
            && error.error.kind() != io::ErrorKind::AlreadyExists
        {
            return Err(E::from(error.error));
        }
    }
    let connection = Connection::open_with_flags(
        database,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    connection.execute_batch("PRAGMA synchronous = FULL; PRAGMA trusted_schema = OFF;")?;
    Ok(connection)
}

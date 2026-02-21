use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Result};

use crate::notifications;

/// Creates the SQLite db file if it doesnt exist, then creates the artists and new releases table.
///
/// ### Errors
///
/// This function will return an error if it cant create a new db file.
pub fn setup_db() -> Result<Connection> {
    let conn = Connection::open("data/spotify_tracker.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS artists (
        spotify_id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        latest_release_date TEXT,
        last_checked TEXT
    )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS new_releases (
        id INTEGER PRIMARY KEY,
        artist_name TEXT,
        album_name TEXT,
        release_date TEXT,
        spotify_url TEXT,
        album_art_url TEXT, -- New column
        sent_at DATETIME DEFAULT NULL
    )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS sync_status (
        key TEXT PRIMARY KEY,
        value TEXT
    )",
        [],
    )?;
    Ok(conn)
}

/// Inserts a new artist into the artists table if they do not already exist.
///
/// This function is used during the library sync process. It sets a baseline
/// check date of 'now' and a release date baseline to prevent the script
/// from back-scanning an artist's entire historical discography on the first run.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
/// * `id` - The unique Spotify ID of the artist.
/// * `name` - The display name of the artist.
///
/// ### Errors
///
/// This function will return an error if the database execution fails,
/// though it will gracefully ignore conflicts if the artist ID already exists.
pub fn add_artist(conn: &Connection, id: &str, name: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO artists (spotify_id, name, last_checked, latest_release_date) 
         VALUES (?1, ?2, datetime('now'), date('now'))",
        [id, name],
    )?;
    Ok(())
}

/// Updates an artist's latest release date and marks them as checked.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
/// * `id` - The unique Spotify ID of the artist.
/// * `date` - The YYYY-MM-DD string of the new release.
///
/// ### Errors
///
/// This function will return an error if the database update execution fails.
pub fn update_artist_release(conn: &Connection, id: &str, date: &str) -> rusqlite::Result<()> {
    notifications::log_and_print(&format!("Newest Release Date: {}", date));
    conn.execute(
        "UPDATE artists 
         SET latest_release_date = ?, 
             last_checked = datetime('now') 
         WHERE spotify_id = ?",
        [date, id],
    )?;
    Ok(())
}

/// Returns the date of the most recent release found for a specific artist.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
/// * `artist_id` - The unique Spotify ID of the artist.
///
/// ### Errors
///
/// This function will return an error if the query fails, though it defaults to an empty string if no record exists.
pub fn get_artist_last_release(
    conn: &rusqlite::Connection,
    artist_id: &str,
) -> rusqlite::Result<String> {
    let mut stmt = conn.prepare("SELECT latest_release_date FROM artists WHERE spotify_id = ?1")?;

    // Attempt to query the date, default to empty string if not found
    let date: Option<String> = stmt.query_row([artist_id], |row| row.get(0)).ok();

    Ok(date.unwrap_or_default())
}

/// Returns artists from the database who have not been checked for new music in the last 24 hours.
///
/// This function prioritizes artists who have never been checked (NULL `last_checked`)
/// or those with the oldest check timestamps, ensuring a rotating queue of 100
/// artists per hourly run.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
/// * `limit` - The maximum number of artists to retrieve (typically 100).
///
/// ### Errors
///
/// This function will return an error if the selection query fails or if the
/// results cannot be mapped to the expected (ID, Name) tuple.
pub fn get_stale_artists(conn: &Connection, limit: i32) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT spotify_id, name FROM artists 
         WHERE last_checked IS NULL 
         OR last_checked < datetime('now', '-24 hours') 
         ORDER BY last_checked ASC 
         LIMIT ?",
    )?;

    let rows = stmt.query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?;

    let mut artists = Vec::new();
    for artist in rows {
        artists.push(artist?);
    }
    Ok(artists)
}

/// Retrieves all release records from the database that have not yet been included in a report.
///
/// This function queries the `new_releases` table for rows where `sent_at` is NULL,
/// providing the data necessary to generate the daily HTML digest.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the SELECT query fails or if the data
/// in the columns cannot be mapped to the return strings.
pub fn get_pending_notifications(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<Vec<(String, String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT artist_name, album_name, spotify_url, album_art_url FROM new_releases WHERE sent_at IS NULL"
    )?;

    let rows = stmt.query_map([], |row| {
        let artist: String = row.get(0)?;
        let album: String = row.get(1)?;
        let url: String = row.get(2)?;
        // Use Option<String> to handle potential NULL values safely
        let art_url: Option<String> = row.get(3)?;

        Ok((artist, album, url, art_url.unwrap_or_default()))
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Inserts a new release entry into the new_releases table to be included in the next report.
///
/// This function acts as a staging area, storing new music found during hourly scans
/// without sending it immediately. The record remains with a NULL `sent_at` value
/// until the daily report is triggered.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
/// * `artist` - The display name of the artist.
/// * `album` - The name of the album or single found.
/// * `date` - The release date string (usually YYYY-MM-DD).
/// * `url` - The direct Spotify URL to the new release.
/// * `art_url` - The URL for the high-resolution album cover art.
///
/// ### Errors
///
/// This function will return an error if the INSERT statement fails, such as during a database lock.
pub fn queue_notification(
    conn: &rusqlite::Connection,
    artist: &str,
    album: &str,
    date: &str,
    url: &str,
    art_url: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO new_releases (artist_name, album_name, release_date, spotify_url, album_art_url) 
         VALUES (?, ?, ?, ?, ?)",
        [artist, album, date, url, art_url],
    )?;
    Ok(())
}

/// Marks all currently unsent notifications in the database as sent.
///
/// This function sets the `sent_at` timestamp for all records where it is currently NULL,
/// effectively clearing the queue for the next daily report.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the update query fails to execute on the new_releases table.
pub fn mark_notifications_as_sent(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE new_releases SET sent_at = datetime('now') WHERE sent_at IS NULL",
        [],
    )?;
    Ok(())
}

/// Sets a cooldown timestamp to ground the script.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
/// * `seconds` - Number of seconds the cooldown should last.
///
/// ### Errors
///
/// This function will return an error if the insertion fails.
pub fn set_cooldown(conn: &Connection, seconds: u64) -> Result<()> {
    // Cast to i64 for SQLite compatibility
    let seconds_i64 = seconds as i64;
    conn.execute(
        "INSERT OR REPLACE INTO sync_status (key, value) 
         VALUES ('cooldown_until', datetime('now', '+' || ?1 || ' seconds'))",
        params![seconds_i64],
    )?;
    Ok(())
}

/// Retrieves the cooldown expiration timestamp if the script is currently grounded.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the query fails to execute.
pub fn get_cooldown_expiry(conn: &Connection) -> Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM sync_status 
         WHERE key = 'cooldown_until' AND value > datetime('now')",
        [],
        |row| row.get(0),
    )
    .optional()
}

/// Retrieves a summary of the synchronization status.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the stat queries fail.
pub fn get_sync_stats(conn: &Connection) -> Result<(i64, i64)> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM artists", [], |r| r.get(0))?;

    let fresh: i64 = conn.query_row(
        "SELECT COUNT(*) FROM artists WHERE last_checked > datetime('now', '-24 hours')",
        [],
        |r| r.get(0),
    )?;

    Ok((total, fresh))
}

/// Synchronizes the local artist database with the current Spotify library.
///
/// ### Arguments
///
/// * `conn` - Mutable database connection object (required for transactions).
/// * `current_ids` - A slice of Spotify IDs currently in the user's library.
///
/// ### Errors
///
/// This function will return an error if the transaction fails or the deletion query encounters an issue.
pub fn reconcile_artists(conn: &mut Connection, current_ids: &[String]) -> Result<usize> {
    let tx = conn.transaction()?;

    // 1. Create a lightweight temporary table
    tx.execute(
        "CREATE TEMPORARY TABLE current_sync (id TEXT PRIMARY KEY)",
        [],
    )?;

    // 2. Bulk insert the live IDs
    let mut stmt = tx.prepare("INSERT INTO current_sync (id) VALUES (?)")?;
    for id in current_ids {
        stmt.execute([id])?;
    }
    drop(stmt); // Close statement before running the next query

    // 3. Delete artists NOT in the temp table
    let deleted_count = tx.execute(
        "DELETE FROM artists WHERE spotify_id NOT IN (SELECT id FROM current_sync)",
        [],
    )?;

    tx.commit()?;
    Ok(deleted_count)
}

/// Retrieves the timestamp of the last generated report from the sync_status table.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the database query fails or the data is corrupted.
pub fn get_last_report_time(conn: &Connection) -> Result<Option<DateTime<Utc>>> {
    let res: Option<String> = conn
        .query_row(
            "SELECT value FROM sync_status WHERE key = 'last_report_sent'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    match res {
        Some(s) => {
            // SQLite format: "YYYY-MM-DD HH:MM:SS"
            // We append " +0000" to tell Chrono it's UTC
            let format = "%Y-%m-%d %H:%M:%S %z";
            let datetime_str = format!("{} +0000", s);
            Ok(DateTime::parse_from_str(&datetime_str, format)
                .map(|dt| dt.with_timezone(&Utc))
                .ok())
        }
        None => Ok(None),
    }
}

/// Updates the 'last_report_sent' key to the current database time.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the update execution fails.
pub fn update_last_report_time(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_status (key, value) 
         VALUES ('last_report_sent', datetime('now'))",
        [],
    )?;
    Ok(())
}

/// Simple count of releases that haven't been included in a report yet.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Errors
///
/// This function will return an error if the count query fails.
pub fn get_pending_notifications_count(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM new_releases WHERE sent_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Retrieves the timestamp of the last library sync from the sync_status table.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
pub fn get_last_library_sync_time(conn: &Connection) -> Result<Option<DateTime<Utc>>> {
    let res: Option<String> = conn
        .query_row(
            "SELECT value FROM sync_status WHERE key = 'last_library_sync'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    match res {
        Some(s) => {
            let format = "%Y-%m-%d %H:%M:%S %z";
            let datetime_str = format!("{} +0000", s);
            Ok(DateTime::parse_from_str(&datetime_str, format)
                .map(|dt| dt.with_timezone(&Utc))
                .ok())
        }
        None => Ok(None),
    }
}

/// Updates the 'last_library_sync' key to the current database time.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
pub fn update_last_library_sync_time(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_status (key, value) 
         VALUES ('last_library_sync', datetime('now'))",
        [],
    )?;
    Ok(())
}

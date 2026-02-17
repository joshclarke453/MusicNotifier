mod db;
mod models;
mod notifications;
mod spotify;

use dotenvy::dotenv;
use rusqlite::Connection;
use std::io::{self, Write};

/// The orchestrator for the Spotify Release Tracker application.
///
/// This function initializes environment variables, sets up the database,
/// and executes the main logic loop. It handles the transition between
/// library syncing, artist scanning, and daily reporting.
///
/// ### Execution Flow
///
/// 1. **Initialization:** Loads `.env` and opens the SQLite connection.
/// 2. **Cooldown Guard:** Exits early if the script is grounded due to rate limits.
/// 3. **Authentication:** Retrieves or refreshes the Spotify Access Token.
/// 4. **Library Sync:** Updates the local artist database from liked songs.
/// 5. **Release Scan:** Iterates through stale artists to find new music.
/// 6. **Reporting:** Triggers the daily HTML digest if the 23-hour window has passed.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    notifications::log_and_print(&format!("--- Spotify Release Tracker: Background Mode ---"));

    notifications::log_and_print(&format!("Starting scan."));

    let mut conn = db::setup_db().expect("Failed to setup database");
    let auth = spotify::SpotifyAuth::new();

    // Ground the script if we are in a cooldown period to avoid API bans
    if let Some(expiry) = db::get_cooldown_expiry(&conn)? {
        notifications::log_and_print(&format!(
            "Script is grounded until {}. Skipping run.",
            expiry
        ));
        return Ok(());
    }

    let token_data = get_token(auth).await;

    let client = spotify::SpotifyClient::new(token_data.access_token);

    sync_library(&mut conn, &client).await;

    let stale_artists = get_stale_artists(&conn);

    let mut count = 0;
    for (id, name) in stale_artists {
        count += 1;
        notifications::log_and_print(&format!("Checking Artist: {}", name));

        let old_date = db::get_artist_last_release(&conn, &id).unwrap_or_default();
        if old_date.is_empty() {
            // Baseline for new artists to prevent flooding with years of history
            let today = "2026-02-15";
            notifications::log_and_print(&format!(
                "First time check for {}. Setting baseline to {}.",
                name, today
            ));
            db::update_artist_release(&conn, &id, today).ok();
            continue;
        }

        if count % 50 == 0 {
            notifications::log_and_print(&format!("Progress: Checked {} artists so far...", count));
        }

        tokio::select! {
            result = client.get_all_new_releases(&id, &old_date) => {
                match result {
                    Ok(new_releases) => {
                        if !new_releases.is_empty() {
                            let newest_release = new_releases.iter()
                                .max_by_key(|r| &r.release_date);

                            if let Some(latest) = newest_release {
                                db::update_artist_release(&conn, &id, &latest.release_date).ok();
                            }

                            for release in new_releases {
                                notifications::log_and_print(&format!("NEW: {} - {} ({})", name, release.name, release.release_date));
                                let art_url = release.images.first().map(|img| img.url.as_str()).unwrap_or("");

                                db::queue_notification(
                                    &conn, &name, &release.name, &release.release_date,
                                    &release.external_urls.spotify, art_url
                                ).ok();
                            }
                        } else {
                            db::update_artist_release(&conn, &id, &old_date).ok();
                        }
                    }
                    Err(e) => {
                        // Handle 429 Too Many Requests by setting a 23-hour cooldown
                        if let Some(req_err) = e.downcast_ref::<reqwest::Error>() {
                            if req_err.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
                                notifications::log_and_print(&format!("Rate limit detected. Grounding script for 23 hours."));
                                db::set_cooldown(&conn, 82800).ok();
                                break;
                            }
                        }
                        // Update last_checked even on failure so we don't get stuck on this artist
                        db::update_artist_release(&conn, &id, &old_date).ok();
                        notifications::log_and_print(&format!("Skip {}: {}", name, e));
                    }
                }
            }
            // Graceful shutdown on Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                notifications::log_and_print(&format!("\n Shutdown signal received. Finalizing..."));
                break;
            }
        }

        // Anti-bot detection: Randomized delay between artist checks
        let sleep_time = 5 + rand::random::<u64>() % 10; // Wait between 5 and 15 seconds
        tokio::time::sleep(tokio::time::Duration::from_secs(sleep_time)).await;
    }

    send_notification(&conn);
    Ok(())
}

/// Orchestrates the retrieval and validation of the Spotify Access Token.
///
/// This function attempts to load saved credentials from disk. If found, it
/// verifies them; if expired, it attempts a refresh. If no credentials exist,
/// it prompts the user for a manual OAuth2 code exchange.
///
/// ### Arguments
///
/// * `auth` - The [`spotify::SpotifyAuth`] configuration object.
///
/// ### Returns
///
/// Returns a valid [`spotify::SpotifyToken`] ready for API use.
async fn get_token(auth: spotify::SpotifyAuth) -> spotify::SpotifyToken {
    let token_data = if let Some(mut saved_tokens) = spotify::SpotifyAuth::load_tokens() {
        let test_client = spotify::SpotifyClient::new(saved_tokens.access_token.clone());
        match test_client.verify_token().await {
            Ok(_) => saved_tokens,
            Err(_) => {
                notifications::log_and_print(&format!("Refreshing access token..."));
                let refresh_val = saved_tokens
                    .refresh_token
                    .as_ref()
                    .expect("No refresh token");
                match auth.refresh_token(refresh_val).await {
                    Ok(new_token_data) => {
                        saved_tokens.update_from_refresh(new_token_data);
                        spotify::SpotifyAuth::save_tokens(&saved_tokens);
                        saved_tokens
                    }
                    Err(_) => {
                        notifications::log_and_print(&format!(
                            "Authorization required: {}",
                            auth.get_authorize_url()
                        ));
                        notifications::log_and_print(&format!("Paste code: "));
                        io::stdout().flush().unwrap();
                        let mut code = String::new();
                        io::stdin().read_line(&mut code).unwrap();
                        let tokens = auth
                            .get_token(code.trim())
                            .await
                            .expect("Failed to get token");
                        spotify::SpotifyAuth::save_tokens(&tokens);
                        tokens
                    }
                }
            }
        }
    } else {
        notifications::log_and_print(&format!(
            "Authorization required: {}",
            auth.get_authorize_url()
        ));
        notifications::log_and_print(&format!("Paste code: "));
        io::stdout().flush().unwrap();
        let mut code = String::new();
        io::stdin().read_line(&mut code).unwrap();
        let tokens = auth
            .get_token(code.trim())
            .await
            .expect("Failed to get token");
        spotify::SpotifyAuth::save_tokens(&tokens);
        tokens
    };
    return token_data;
}

/// Syncs the local database with the user's current Spotify liked artists.
///
/// This ensures that the tracker stays up to date as the user likes or unlikes
/// songs on Spotify. It adds new artists and removes those no longer present.
///
/// ### Arguments
///
/// * `conn` - A mutable reference to the database connection.
/// * `client` - A reference to the authenticated [`spotify::SpotifyClient`].
async fn sync_library(conn: &mut Connection, client: &spotify::SpotifyClient) {
    notifications::log_and_print(&format!("Syncing library..."));
    if let Ok(spotify_artists) = client.get_liked_artists().await {
        let current_ids: Vec<String> = spotify_artists.iter().map(|a| a.id.clone()).collect();

        for artist in &spotify_artists {
            db::add_artist(&conn, &artist.id, &artist.name).ok();
        }

        if !current_ids.is_empty() {
            match db::reconcile_artists(conn, &current_ids) {
                Ok(count) if count > 0 => {
                    notifications::log_and_print(&format!(
                        "Purged {} artists no longer in your library.",
                        count
                    ));
                }
                Ok(_) => notifications::log_and_print(&format!("Database is perfectly in sync.")),
                Err(e) => notifications::log_and_print(&format!("Sync cleanup failed: {}", e)),
            }
        }
    }
}

/// Retrieves a list of artists that have not been checked for releases recently.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
///
/// ### Returns
///
/// A vector of tuples containing (Spotify ID, Artist Name).
fn get_stale_artists(conn: &Connection) -> Vec<(std::string::String, std::string::String)> {
    let stale_artists = db::get_stale_artists(&conn, 100).expect("DB Error");
    notifications::log_and_print(&format!(
        "Checking {} artists for new music...",
        stale_artists.len()
    ));
    return stale_artists;
}

/// Evaluates if a daily report should be sent and generates the HTML digest.
///
/// This function checks the `sync_status` table for the last report timestamp.
/// If a report is dispatched, it updates the database to mark releases as sent.
///
/// ### Arguments
///
/// * `conn` - Database connection object.
fn send_notification(conn: &Connection) {
    let last_report = db::get_last_report_time(&conn).unwrap_or(None);
    let now = chrono::Utc::now();

    let should_send_report = match last_report {
        Some(time) => (now - time).num_hours() >= 23,
        None => true, // Send immediately if it's the first time ever
    };

    if should_send_report {
        let pending = db::get_pending_notifications(&conn).expect("DB Error");

        if !pending.is_empty() {
            notifications::log_and_print(&format!(
                "Daily Digest: {} new releases found!",
                pending.len()
            ));
            let html = notifications::format_release_html(pending);

            match notifications::save_report_to_file(html) {
                Ok(path) => {
                    notifications::log_and_print(&format!("Daily Report saved to: {}", path));
                    db::mark_notifications_as_sent(&conn).ok();
                    db::update_last_report_time(&conn).ok();
                }
                Err(e) => notifications::log_and_print(&format!("Failed to save report: {}", e)),
            }
        } else {
            // Even if no new songs were found, update the time so we don't
            // keep checking "pending" every single hour today.
            notifications::log_and_print("No new releases for today's digest.");
            db::update_last_report_time(&conn).ok();
        }
    } else {
        let pending_count = db::get_pending_notifications_count(&conn).unwrap_or(0);
        notifications::log_and_print(&format!(
            "Hourly check complete. {} releases queued for next daily report.",
            pending_count
        ));
    }

    let (total, fresh) = db::get_sync_stats(&conn).unwrap_or((0, 0));

    notifications::log_and_print(&format!(
        "Completed scan. {}/{} artists are now fresh.",
        fresh, total
    ));
}

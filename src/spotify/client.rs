use crate::{
    models::{
        self, Album, Artist, ArtistAlbumsResponse, LikedSongsResponse, SpotifyRateLimitError,
    },
    notifications,
};
use reqwest::header::{HeaderValue, ACCEPT_LANGUAGE, AUTHORIZATION, USER_AGENT};
use std::collections::HashSet;

/// A client for interacting with the Spotify Web API.
///
/// This client handles fetching artist discographies, verifying authentication,
/// and synchronizing the user's followed artists based on their liked songs.
pub struct SpotifyClient {
    pub access_token: String,
    client: reqwest::Client,
}

impl SpotifyClient {
    /// Creates a new [`SpotifyClient`] with the provided access token.
    ///
    /// ### Arguments
    ///
    /// * `access_token` - A valid Spotify OAuth access token.
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            client: reqwest::Client::builder()
                .user_agent("MySpotifyTracker/1.0 (PersonalProject)")
                .build()
                .unwrap_or_default(),
        }
    }

    /// Fetches all new albums and singles for a given artist since a specific date.
    ///
    /// This function aggregates results from multiple release groups and removes
    /// duplicates that may appear across different categories.
    ///
    /// ### Arguments
    ///
    /// * `artist_id` - The Spotify ID of the artist.
    /// * `last_checked_date` - The YYYY-MM-DD baseline date.
    ///
    /// ### Errors
    ///
    /// Returns an error if any network request fails or if the API returns a rate-limit status.
    pub async fn get_all_new_releases(
        &self,
        artist_id: &str,
        latest_release_date: &str,
    ) -> Result<Vec<Album>, Box<dyn std::error::Error>> {
        let mut all_found = Vec::new();
        let groups = vec!["album", "single"]; //"appears_on" add this back to the list once I get a solid db going

        for group in groups {
            let group_releases = self
                .fetch_group_releases(artist_id, group, latest_release_date)
                .await?;
            all_found.extend(group_releases);
        }

        all_found.sort_by(|a, b| b.release_date.cmp(&a.release_date));

        let mut seen = HashSet::new();
        all_found.retain(|album| seen.insert(album.id.clone()));

        Ok(all_found)
    }

    /// Internal helper to fetch a specific group (album/single) of releases.
    ///
    /// Performs pagination and filters out records older than the `last_checked_date`.
    ///
    /// ### Arguments
    ///
    /// * `artist_id` - The Spotify ID of the artist.
    /// * `group` - The release group type (e.g., "album").
    /// * `last_checked_date` - The YYYY-MM-DD cutoff date.
    async fn fetch_group_releases(
        &self,
        artist_id: &str,
        group: &str,
        latest_release_date: &str,
    ) -> Result<Vec<Album>, Box<dyn std::error::Error>> {
        let mut group_found = Vec::new();
        let mut next_url = Some(format!(
            "https://api.spotify.com/v1/artists/{}/albums?limit=10&include_groups={}",
            artist_id, group
        ));

        while let Some(ref url) = next_url {
            let res = self.client
                .get(url)
                .header(AUTHORIZATION, format!("Bearer {}", self.access_token))
                .header(
                    USER_AGENT,
                    HeaderValue::from_static(
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"
                    )
                )
                .header(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"))
                .send().await?;

            let status = res.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let wait_seconds = res
                    .headers()
                    .get("Retry-After")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(3600); // Default to 1 hour if header is missing

                return Err(Box::new(SpotifyRateLimitError {
                    retry_after: wait_seconds + 1,
                }));
            }

            if !status.is_success() {
                let err_body = res.text().await?;
                return Err(format!("API Error: Status {}. Body: {}", status, err_body).into());
            }

            let text = res.text().await?;

            let data: ArtistAlbumsResponse = match serde_json::from_str(&text) {
                Ok(d) => d,
                Err(e) => {
                    notifications::log_and_print(&format!("JSON Error: {}", e));
                    notifications::log_and_print(&format!("RAW BODY: {}", text));
                    return Err(e.into());
                }
            };

            if data.items.is_empty() {
                break;
            }

            let mut page_has_old_content = false;

            for album in data.items {
                if album.release_date <= latest_release_date.to_string() {
                    page_has_old_content = true;
                    continue;
                }

                if album.album_type == "compilation" {
                    continue;
                }

                let is_actually_on_it = album.artists.iter().any(|a| a.id == artist_id);
                if is_actually_on_it {
                    group_found.push(album);
                }
            }

            if !page_has_old_content && data.next.is_some() {
                next_url = data.next;
            } else {
                next_url = None;
            }

            // Anti-bot detection: Randomized delay between artist checks
            let sleep_time = 10 + (rand::random::<u64>() % 15); // Wait between 5 and 15 seconds
            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_time)).await;
        }
        Ok(group_found)
    }

    /// Verifies that the current access token is still valid.
    ///
    /// ### Errors
    ///
    /// Returns an error if the token has expired or is invalid.
    pub async fn verify_token(&self) -> Result<(), reqwest::Error> {
        self.client
            .get("https://api.spotify.com/v1/me")
            .header(AUTHORIZATION, format!("Bearer {}", self.access_token))
            .header(
                USER_AGENT,
                HeaderValue::from_static(
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"
                )
            )
            .header(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"))
            .send().await?
            .error_for_status()?;
        Ok(())
    }

    /// Scans the user's Liked Songs to extract a list of unique artists.
    ///
    /// This is used to build the local database of artists to track for new releases.
    ///
    /// ### Errors
    ///
    /// Returns an error if the network request fails or if Spotify returns a 429 Rate Limit.
    pub async fn get_liked_artists(&self) -> Result<Vec<Artist>, Box<dyn std::error::Error>> {
        let mut artists = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        let mut next_url = Some("https://api.spotify.com/v1/me/tracks?limit=50".to_string());

        while let Some(url) = next_url {
            let response = self.client
                .get(&url)
                .header(AUTHORIZATION, format!("Bearer {}", self.access_token))
                .header(
                    USER_AGENT,
                    HeaderValue::from_static(
                        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36"
                    )
                )
                .header(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"))
                .send().await?;

            if !response.status().is_success() {
                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    let wait_seconds = response
                        .headers()
                        .get("Retry-After")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(3600);
                    return Err(Box::new(models::SpotifyRateLimitError {
                        retry_after: wait_seconds + 1,
                    }));
                } else {
                    notifications::log_and_print(&format!(
                        "Sync Error: Spotify returned status {}",
                        response.status()
                    ));
                }
                return Err(response.error_for_status().unwrap_err().into());
            }

            let data = response.json::<LikedSongsResponse>().await?;

            for item in data.items {
                for artist in item.track.artists {
                    if !seen_ids.contains(&artist.id) {
                        seen_ids.insert(artist.id.clone());
                        artists.push(artist);
                    }
                }
            }

            next_url = data.next;

            let sleep_time = 3 + (rand::random::<u64>() % 2); // Wait between 3 and 5 seconds
            tokio::time::sleep(tokio::time::Duration::from_secs(sleep_time)).await;

            notifications::log_and_print(&format!(
                "Checked a page... found {} unique artists so far",
                artists.len()
            ));
        }

        Ok(artists)
    }
}

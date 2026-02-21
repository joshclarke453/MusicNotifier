use serde::{Deserialize, Serialize};
use std::fmt;

/// The top-level response from GET /me/tracks
#[derive(Debug, Deserialize, Serialize)]
pub struct LikedSongsResponse {
    pub items: Vec<SavedTrack>,
    pub next: Option<String>, // URL for the next page of songs
    pub total: u32,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SavedTrack {
    pub added_at: String,
    pub track: Track,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub artists: Vec<Artist>,
    pub album: Album,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ExternalUrls {
    pub spotify: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub release_date: String,
    pub release_date_precision: String, // "year", "month", or "day"
    pub album_group: Option<String>,
    pub images: Vec<SpotifyImage>,
    pub external_urls: ExternalUrls,
    pub artists: Vec<Artist>,
    pub total_tracks: u32,
    pub album_type: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ArtistAlbumsResponse {
    pub items: Vec<Album>,
    pub next: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SpotifyImage {
    pub url: String,
    pub height: Option<i32>,
    pub width: Option<i32>,
}

#[derive(Debug)]
pub struct SpotifyRateLimitError {
    pub retry_after: u64,
}

impl fmt::Display for SpotifyRateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Spotify Rate Limit: Retry after {}s", self.retry_after)
    }
}

impl std::error::Error for SpotifyRateLimitError {}

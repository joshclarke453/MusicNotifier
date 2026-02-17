use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{env, fs};

#[derive(Debug, Deserialize, Serialize)]
pub struct SpotifyToken {
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    pub expires_in: i32,
    pub refresh_token: Option<String>,
}

pub struct SpotifyAuth {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl SpotifyToken {
    /// Updates the current token fields with data from a refresh response.
    ///
    /// This method ensures that the critical `refresh_token` is preserved if the 
    /// Spotify API response doesn't provide a new one.
    ///
    /// ### Arguments
    ///
    /// * `new_data` - The new [`SpotifyToken`] received from the refresh flow.
    pub fn update_from_refresh(&mut self, new_data: SpotifyToken) {
        self.access_token = new_data.access_token;
        self.expires_in = new_data.expires_in;
        self.scope = new_data.scope;
        // ONLY update refresh_token if the new data actually contains one
        if new_data.refresh_token.is_some() {
            self.refresh_token = new_data.refresh_token;
        }
    }
}

impl SpotifyAuth {

    /// Creates a new [`SpotifyAuth`] by loading credentials from environment variables.
    ///
    /// ### Arguments
    ///
    /// * N/A - Loads from `SPOTIFY_CLIENT_ID`, `SPOTIFY_CLIENT_SECRET`, and `SPOTIFY_REDIRECT_URI`.
    ///
    /// ### Panics
    ///
    /// Panics if Client id, secret or redirect_url arent set in the environment.
    pub fn new() -> Self {
        Self {
            client_id: env::var("SPOTIFY_CLIENT_ID").expect("ID not set"),
            client_secret: env::var("SPOTIFY_CLIENT_SECRET").expect("Secret not set"),
            redirect_uri: env::var("SPOTIFY_REDIRECT_URI").expect("URI not set"),
        }
    }

    /// Returns the Spotify authorization URL used to prompt user login.
    ///
    /// This URL requests the `user-library-read` and `user-follow-read` scopes.
    pub fn get_authorize_url(&self) -> String {
        let scopes = "user-library-read user-follow-read"; 
        
        format!(
            "https://accounts.spotify.com/authorize?client_id={}&response_type=code&scope={}&redirect_uri={}&show_dialog=true",
            self.client_id, 
            scopes.replace(" ", "%20"), 
            self.redirect_uri
        )
    }

    /// Exchanges an authorization code for a [`SpotifyToken`].
    /// 
    /// ### Arguments
    /// 
    /// * `code` - The authorization code returned by Spotify after user login.
    ///
    /// ### Errors
    ///
    /// This function will return an error if the network request fails or if Spotify 
    /// returns an invalid JSON response.
    pub async fn get_token(&self, code: &str) -> Result<SpotifyToken, reqwest::Error> {
        let client = Client::new();
        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.redirect_uri),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        let response = client
            .post("https://accounts.spotify.com/api/token")
            .form(&params)
            .send()
            .await?;

        response.json::<SpotifyToken>().await
    }
    
    /// Serializes and saves the provided tokens to `credentials.json`.
    ///
    /// ### Arguments
    ///
    /// * `tokens` - A reference to the [`SpotifyToken`] to be saved.
    ///
    /// ### Panics
    ///
    /// Panics if the file cannot be written to the disk.
    pub fn save_tokens(tokens: &SpotifyToken) {
        let json = serde_json::to_string(tokens).unwrap();
        fs::write("credentials.json", json).expect("Unable to save tokens");
    }

    /// Attempts to load and deserialize tokens from `credentials.json`.
    ///
    /// Returns `None` if the file is missing or contains invalid data.
    pub fn load_tokens() -> Option<SpotifyToken> {
        let data = fs::read_to_string("credentials.json").ok()?;
        serde_json::from_str(&data).ok()
    }

    /// Uses a refresh token to obtain a new access token from Spotify.
    ///
    /// ### Arguments
    ///
    /// * `refresh_token` - The long-lived refresh token string.
    ///
    /// ### Errors
    ///
    /// This function will return an error if the request fails or the refresh token is revoked/invalid.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<SpotifyToken, reqwest::Error> {
        let client = reqwest::Client::new();
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
        ];

        client.post("https://developer.spotify.com/documentation/web-api/tutorials/code-flow5")
            .form(&params)
            .send()
            .await?
            .json::<SpotifyToken>()
            .await
    }

}
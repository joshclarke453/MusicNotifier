Spotify essentially killed this project with their February 2026 API changes. It technically works, but will receive constant 24hr rate limit timeouts from spotify which are unavoidable even when putting in massive delays between API calls. Seems like Spotify has either implemented some form of scraping detection which this program triggers, or a set limit on how many API requests a program in development mode can make a day, and to not be indevelopment mode, you need 250,000 active users, which is unachievable for this project.

🎧 Spotify Release Tracker:

A background service built in Rust that monitors followed artists and generates local HTML reports when new music is released. Designed to run silently on system startup without hitting Spotify's rate limits.

🚀 How It Works:

Syncs your liked artists to a local SQLite database.

Polls artists in "stale" order (the ones not checked in the longest time).

Throttles requests (5s delay) to remain invisible to Spotify's anti-bot measures.

Generates a timestamped HTML report in /reports only if new music is found.

Logs all background activity to run_log.txt.

🛠 Setup (Local Only):
1. Prerequisites
- Rust (Stable 1.85+ for Edition 2024 support)
- A Spotify Developer Account and a registered App.

2. Environment Variables:
   
- Create a .env file in the project root directory (this is gitignored for security):
- SPOTIFY_CLIENT_ID=your_id_here
- SPOTIFY_CLIENT_SECRET=your_secret_here
- REDIRECT_URI=http://localhost:8080

3. Running the Service:

To run manually:

    Bash
    cargo run --release

To run as a background task on Windows:
    Build the release executable: cargo build --release.
    Create a .bat file to run 
        cargo run --release
    Create a new task in Windows Task Scheduler pointing at ther .bat file.
    Set the trigger to "At Log on".

🛡 Security Notes:

.env and credentials.json contain private API keys and tokens. NEVER commit these to version control.
The database is kept local to protect your private listening habits and artist list.

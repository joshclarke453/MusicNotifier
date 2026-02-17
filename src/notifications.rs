use chrono::Local;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// Generates a styled HTML string containing a list of new music releases.
///
/// This function constructs a mobile-friendly, modern HTML email-style layout
/// featuring album art, artist names, album titles, and direct Spotify links.
///
/// ### Arguments
///
/// * `releases` - A vector of tuples containing (Artist Name, Album Name, Spotify URL, Art URL).
pub fn format_release_html(releases: Vec<(String, String, String, String)>) -> String {
    let mut html = String::from(
        "<html><body style='font-family: sans-serif; background-color: #f4f4f4; padding: 20px;'>\
         <div style='max-width: 600px; margin: auto; background: white; padding: 20px; border-radius: 10px; shadow: 0 4px 6px rgba(0,0,0,0.1);'>\
         <h2 style='color: #1DB954; text-align: center;'>🎧 New Music Found!</h2>"
    );

    for (artist, album, url, art_url) in releases {
        html.push_str(&format!(
            "<div style='display: flex; align-items: center; border-bottom: 1px solid #eee; padding: 15px 0;'>\
                <img src='{}' style='width: 80px; height: 80px; border-radius: 4px; margin-right: 15px; object-fit: cover;' />\
                <div style='flex-grow: 1;'>\
                    <strong style='font-size: 16px;'>{}</strong><br/>\
                    <span style='color: #666;'>{}</span>\
                </div>\
                <a href='{}' style='background-color: #1DB954; color: white; padding: 10px 16px; text-decoration: none; border-radius: 20px; font-size: 13px; font-weight: bold;'>Listen</a>\
            </div>",
            art_url, artist, album, url
        ));
    }

    html.push_str("</div></body></html>");
    html
}

/// Saves the generated HTML report to the local 'reports' directory with a timestamp.
///
/// ### Arguments
///
/// * `html` - The full HTML content string to be saved.
///
/// ### Errors
///
/// This function will return an error if the 'reports' directory cannot be created
/// or if writing the file to disk fails.
pub fn save_report_to_file(html: String) -> std::io::Result<String> {
    let dir = "reports";
    if !Path::new(dir).exists() {
        fs::create_dir(dir)?;
    }

    let date = Local::now().format("%Y-%m-%d_%H-%M").to_string();
    let file_path = format!("{}/report_{}.html", dir, date);

    fs::write(&file_path, html)?;

    Ok(file_path)
}

/// Appends a timestamped message to the local run log file.
///
/// ### Arguments
///
/// * `message` - The text content to record in the log.
pub fn log_status(message: &str) {
    let date = Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("logs/run_log.txt")
    {
        let _ = writeln!(file, "[{}] {}", date, message);
    }
}

/// Outputs a message to the console and simultaneously records it in the log file.
///
/// ### Arguments
///
/// * `message` - The text content to display and log.
pub fn log_and_print(message: &str) {
    println!("{}", message);
    log_status(message);
}

#![deny(warnings)]

use core::result::Result;
use std::cmp::min;
use std::fs::File;
use std::io::Write;
use std::path::Path;

use chrono::prelude::{DateTime, Utc};
use eyre::Report;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;

pub fn iso8601(st: &std::time::SystemTime) -> String {
    let dt: DateTime<Utc> = (*st).into();
    format!("{}", dt.format("%+"))
    // formats like "2001-07-08T00:34:60.026490+09:30"
}

pub async fn download(url: &str, filepath: &Path) -> Result<(), Report> {
    let client = &Client::new();

    let res = client.get(url).send().await?;

    assert_eq!(res.status(), 200);

    let total_size = res
        .content_length()
        .ok_or_else(|| eyre::format_err!("Failed to get content length from {}", &url))?;

    // Indicatif setup
    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
    .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
    .progress_chars("#>-"));
    pb.set_message(format!("Downloading {}", url));

    let mut dest = File::create(filepath)?;

    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|_| eyre::format_err!("Error while downloading file"))?;

        dest.write_all(&chunk)
            .map_err(|_| eyre::format_err!("Error while write file"))?;

        downloaded = min(downloaded + (chunk.len() as u64), total_size);

        pb.set_position(downloaded);
    }

    pb.finish_with_message(format!(
        "Downloaded {} to {}",
        url,
        filepath.as_os_str().to_str().unwrap()
    ));

    Ok(())
}

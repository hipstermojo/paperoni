use std::fs;

use chrono::{DateTime, Local};
use colored::*;
use comfy_table::presets::UTF8_HORIZONTAL_BORDERS_ONLY;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};
use flexi_logger::LevelFilter;
use log::error;

use crate::errors::PaperoniError;

pub fn display_summary(
    initial_article_count: usize,
    succesful_articles_table: Table,
    partial_downloads: Vec<PartialDownload>,
    errors: Vec<PaperoniError>,
) {
    let partial_downloads_count = partial_downloads.len();
    let successfully_downloaded_count =
        initial_article_count - partial_downloads_count - errors.len();

    println!(
        "{}",
        short_summary(DownloadCount::new(
            initial_article_count,
            successfully_downloaded_count,
            partial_downloads_count,
            errors.len()
        ))
        .bold()
    );

    if successfully_downloaded_count > 0 {
        println!("{}", succesful_articles_table);
    }

    if partial_downloads_count > 0 {
        println!("\n{}", "Partially failed downloads".yellow().bold());
        let mut table_partial = Table::new();
        table_partial
            .load_preset(UTF8_HORIZONTAL_BORDERS_ONLY)
            .set_header(vec![
                Cell::new("Link").set_alignment(CellAlignment::Center),
                Cell::new("Title").set_alignment(CellAlignment::Center),
            ])
            .set_content_arrangement(ContentArrangement::Dynamic);

        for partial in partial_downloads {
            table_partial.add_row(vec![&partial.link, &partial.title]);
        }
        println!("{}", table_partial);
    }

    if !errors.is_empty() {
        println!("\n{}", "Failed article downloads".bright_red().bold());
        let mut table_failed = Table::new();
        table_failed
            .load_preset(UTF8_HORIZONTAL_BORDERS_ONLY)
            .set_header(vec![
                Cell::new("Link").set_alignment(CellAlignment::Center),
                Cell::new("Reason").set_alignment(CellAlignment::Center),
            ])
            .set_content_arrangement(ContentArrangement::Dynamic);

        for error in errors {
            let error_source = error
                .article_source()
                .clone()
                .unwrap_or_else(|| "<unknown link>".to_string());
            table_failed.add_row(vec![&error_source, &format!("{}", error.kind())]);
            error!("{}\n - {}", error, error_source);
        }
        println!("{}", table_failed);
    }
}

/// Returns a string summary of the total number of failed and successful article downloads
fn short_summary(download_count: DownloadCount) -> String {
    if download_count.total
        != download_count.successful + download_count.failed + download_count.partial
    {
        panic!("initial_count must be equal to the sum of failed and successful count")
    }
    let get_noun = |count: usize| if count == 1 { "article" } else { "articles" };
    let get_summary = |count, label, color: Color| {
        if count == 0 {
            return "".to_string();
        };

        {
            if count == 1 && count == download_count.total {
                "Article".to_string() + label
            } else if count == download_count.total {
                "All ".to_string() + get_noun(count) + label
            } else {
                count.to_string() + " " + get_noun(count) + label
            }
        }
        .color(color)
        .to_string()
    };

    let mut summary = get_summary(
        download_count.successful,
        " downloaded successfully",
        Color::BrightGreen,
    );

    let partial_summary = get_summary(
        download_count.partial,
        " partially failed to download",
        Color::Yellow,
    );

    if !summary.is_empty() && !partial_summary.is_empty() {
        summary = summary + ", " + &partial_summary;
    } else {
        summary = summary + &partial_summary;
    }

    let failed_summary = get_summary(download_count.failed, " failed to download", Color::Red);
    if !summary.is_empty() && !failed_summary.is_empty() {
        summary = summary + ", " + &failed_summary;
    } else {
        summary = summary + &failed_summary;
    }
    summary
}

struct DownloadCount {
    total: usize,
    successful: usize,
    partial: usize,
    failed: usize,
}
impl DownloadCount {
    fn new(total: usize, successful: usize, partial: usize, failed: usize) -> Self {
        Self {
            total,
            successful,
            partial,
            failed,
        }
    }
}

use crate::errors::LogError as Error;
use crate::http::PartialDownload;

pub fn init_logger(
    log_level: LevelFilter,
    start_time: &DateTime<Local>,
    is_logging_to_file: bool,
) -> Result<(), Error> {
    use directories::UserDirs;
    use flexi_logger::LogSpecBuilder;

    match UserDirs::new() {
        Some(user_dirs) => {
            let home_dir = user_dirs.home_dir();
            let paperoni_dir = home_dir.join(".paperoni");
            let log_dir = paperoni_dir.join("logs");

            let log_spec = LogSpecBuilder::new().module("paperoni", log_level).build();
            let formatted_timestamp = start_time.format("%Y-%m-%d_%H-%M-%S");
            let mut logger = flexi_logger::Logger::with(log_spec);

            if is_logging_to_file {
                if !paperoni_dir.is_dir() || !log_dir.is_dir() {
                    fs::create_dir_all(&log_dir)?;
                }
                logger = logger
                    .directory(log_dir)
                    .discriminant(formatted_timestamp.to_string())
                    .suppress_timestamp()
                    .log_to_file();
            }
            logger.start()?;
            Ok(())
        }
        None => Err(Error::UserDirectoriesError),
    }
}

#[cfg(test)]
mod tests {
    use super::{short_summary, DownloadCount};
    use colored::*;
    #[test]
    fn test_short_summary() {
        assert_eq!(
            short_summary(DownloadCount::new(1, 1, 0, 0)),
            "Article downloaded successfully".bright_green().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(1, 0, 0, 1)),
            "Article failed to download".red().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 10, 0, 0)),
            "All articles downloaded successfully"
                .bright_green()
                .to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 0, 0, 10)),
            "All articles failed to download".red().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 8, 0, 2)),
            format!(
                "{}, {}",
                "8 articles downloaded successfully".bright_green(),
                "2 articles failed to download".red()
            )
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 1, 0, 9)),
            format!(
                "{}, {}",
                "1 article downloaded successfully".bright_green(),
                "9 articles failed to download".red()
            )
        );
        assert_eq!(
            short_summary(DownloadCount::new(7, 6, 0, 1)),
            format!(
                "{}, {}",
                "6 articles downloaded successfully".bright_green(),
                "1 article failed to download".red()
            )
        );
        assert_eq!(
            short_summary(DownloadCount::new(7, 4, 2, 1)),
            format!(
                "{}, {}, {}",
                "4 articles downloaded successfully".bright_green(),
                "2 articles partially failed to download".yellow(),
                "1 article failed to download".red()
            )
        );
        assert_eq!(
            short_summary(DownloadCount::new(12, 6, 6, 0)),
            format!(
                "{}, {}",
                "6 articles downloaded successfully".bright_green(),
                "6 articles partially failed to download".yellow()
            )
        );
        assert_eq!(
            short_summary(DownloadCount::new(5, 0, 4, 1)),
            format!(
                "{}, {}",
                "4 articles partially failed to download".yellow(),
                "1 article failed to download".red()
            )
        );
        assert_eq!(
            short_summary(DownloadCount::new(4, 0, 4, 0)),
            "All articles partially failed to download"
                .yellow()
                .to_string()
        );
    }

    #[test]
    #[should_panic(
        expected = "initial_count must be equal to the sum of failed and successful count"
    )]
    fn test_short_summary_panics_on_invalid_input() {
        short_summary(DownloadCount::new(0, 12, 0, 43));
    }
}

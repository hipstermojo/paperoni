use colored::*;
use comfy_table::presets::UTF8_HORIZONTAL_BORDERS_ONLY;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};
use directories::UserDirs;
use flexi_logger::LogSpecBuilder;
use log::error;

use crate::{cli::AppConfig, errors::PaperoniError};

pub fn display_summary(
    initial_article_count: usize,
    succesful_articles_table: Table,
    partial_downloads_count: usize,
    errors: Vec<PaperoniError>,
) {
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
    // TODO: Refactor this
    if download_count.total
        != download_count.successful + download_count.failed + download_count.partial
    {
        panic!("initial_count must be equal to the sum of failed and successful count")
    }
    let get_noun = |count: usize| if count == 1 { "article" } else { "articles" };
    if download_count.successful == download_count.total && download_count.successful == 1 {
        "Article downloaded successfully".green().to_string()
    } else if download_count.total == download_count.failed && download_count.failed == 1 {
        "Article failed to download".red().to_string()
    } else if download_count.total == download_count.partial && download_count.partial == 1 {
        "Article partially failed to download".yellow().to_string()
    } else if download_count.successful == download_count.total {
        "All articles downloaded successfully".green().to_string()
    } else if download_count.failed == download_count.total {
        "All articles failed to download".red().to_string()
    } else if download_count.partial == download_count.total {
        "All articles partially failed to download"
            .yellow()
            .to_string()
    } else if download_count.partial == 0 {
        format!(
            "{} {} downloaded successfully, {} {} failed",
            download_count.successful,
            get_noun(download_count.successful),
            download_count.failed,
            get_noun(download_count.failed)
        )
        .yellow()
        .to_string()
    } else if download_count.successful == 0
        && download_count.partial > 0
        && download_count.failed > 0
    {
        format!(
            "{} {} partially failed to download, {} {} failed",
            download_count.partial,
            get_noun(download_count.partial),
            download_count.failed,
            get_noun(download_count.failed)
        )
        .yellow()
        .to_string()
    } else if download_count.failed == 0
        && download_count.successful > 0
        && download_count.partial > 0
    {
        format!(
            "{} {} downloaded successfully, {} {} partially failed to download",
            download_count.successful,
            get_noun(download_count.successful),
            download_count.partial,
            get_noun(download_count.partial)
        )
        .yellow()
        .to_string()
    } else {
        format!(
            "{} {} downloaded successfully, {} {} partially failed to download, {} {} failed",
            download_count.successful,
            get_noun(download_count.successful),
            download_count.partial,
            get_noun(download_count.partial),
            download_count.failed,
            get_noun(download_count.failed)
        )
        .yellow()
        .to_string()
    }
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

pub fn init_logger(app_config: &AppConfig) {
    match UserDirs::new() {
        Some(user_dirs) => {
            let home_dir = user_dirs.home_dir();
            let paperoni_dir = home_dir.join(".paperoni");
            let log_dir = paperoni_dir.join("logs");

            let log_spec = LogSpecBuilder::new()
                .module("paperoni", app_config.log_level())
                .build();
            let formatted_timestamp = app_config.start_time().format("%Y-%m-%d_%H-%M-%S");
            let mut logger = flexi_logger::Logger::with(log_spec);

            if app_config.is_logging_to_file() && (!paperoni_dir.is_dir() || !log_dir.is_dir()) {
                match std::fs::create_dir_all(&log_dir) {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("Unable to create paperoni directories on home directory for logging purposes\n{}",e);
                        std::process::exit(1);
                    }
                };
            }

            if app_config.is_logging_to_file() {
                logger = logger
                    .directory(log_dir)
                    .discriminant(formatted_timestamp.to_string())
                    .suppress_timestamp()
                    .log_to_file();
            }

            match logger.start() {
                Ok(_) => (),
                Err(e) => eprintln!("Unable to start logger!\n{}", e),
            }
        }
        None => eprintln!("Unable to get user directories for logging purposes"),
    };
}

#[cfg(test)]
mod tests {
    use super::{short_summary, DownloadCount};
    use colored::*;
    #[test]
    fn test_short_summary() {
        assert_eq!(
            short_summary(DownloadCount::new(1, 1, 0, 0)),
            "Article downloaded successfully".green().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(1, 0, 0, 1)),
            "Article failed to download".red().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 10, 0, 0)),
            "All articles downloaded successfully".green().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 0, 0, 10)),
            "All articles failed to download".red().to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 8, 0, 2)),
            "8 articles downloaded successfully, 2 articles failed"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(10, 1, 0, 9)),
            "1 article downloaded successfully, 9 articles failed"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(7, 6, 0, 1)),
            "6 articles downloaded successfully, 1 article failed"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(7, 4, 2, 1)),
            "4 articles downloaded successfully, 2 articles partially failed to download, 1 article failed"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(12, 6, 6, 0)),
            "6 articles downloaded successfully, 6 articles partially failed to download"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(DownloadCount::new(5, 0, 4, 1)),
            "4 articles partially failed to download, 1 article failed"
                .yellow()
                .to_string()
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

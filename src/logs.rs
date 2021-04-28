use colored::*;
use comfy_table::presets::UTF8_HORIZONTAL_BORDERS_ONLY;
use comfy_table::{Cell, CellAlignment, ContentArrangement, Table};
use directories::UserDirs;
use log::error;

use crate::errors::PaperoniError;

pub fn display_summary(
    initial_article_count: usize,
    succesful_articles_table: Table,
    errors: Vec<PaperoniError>,
) {
    let successfully_downloaded_count = initial_article_count - errors.len();

    println!(
        "{}",
        short_summary(
            initial_article_count,
            successfully_downloaded_count,
            errors.len()
        )
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
fn short_summary(initial_count: usize, successful_count: usize, failed_count: usize) -> String {
    if initial_count != successful_count + failed_count {
        panic!("initial_count must be equal to the sum of failed and successful count")
    }
    let get_noun = |count: usize| if count == 1 { "article" } else { "articles" };
    if successful_count == initial_count && successful_count == 1 {
        "Article downloaded successfully".green().to_string()
    } else if initial_count == failed_count && failed_count == 1 {
        "Article failed to download".red().to_string()
    } else if successful_count == initial_count {
        "All articles downloaded successfully".green().to_string()
    } else if successful_count == 0 {
        "All articles failed to download".red().to_string()
    } else {
        format!(
            "{} {} downloaded successfully, {} {} failed",
            successful_count,
            get_noun(successful_count),
            failed_count,
            get_noun(failed_count)
        )
        .yellow()
        .to_string()
    }
}

pub fn init_logger() {
    match UserDirs::new() {
        Some(user_dirs) => {
            let home_dir = user_dirs.home_dir();
            let paperoni_dir = home_dir.join(".paperoni");
            let log_dir = paperoni_dir.join("logs");
            if !paperoni_dir.is_dir() || !log_dir.is_dir() {
                std::fs::create_dir_all(&log_dir).expect(
                    "Unable to create paperoni directories on home directory for logging purposes",
                );
            }
            match flexi_logger::Logger::with_str("paperoni=debug")
                .directory(log_dir)
                .log_to_file()
                .print_message()
                .start()
            {
                Ok(_) => (),
                Err(e) => eprintln!("Unable to start logger!\n{}", e),
            }
        }
        None => eprintln!("Unable to get user directories for logging purposes"),
    };
}

#[cfg(test)]
mod tests {
    use super::short_summary;
    use colored::*;
    #[test]
    fn test_short_summary() {
        assert_eq!(
            short_summary(1, 1, 0),
            "Article downloaded successfully".green().to_string()
        );
        assert_eq!(
            short_summary(1, 0, 1),
            "Article failed to download".red().to_string()
        );
        assert_eq!(
            short_summary(10, 10, 0),
            "All articles downloaded successfully".green().to_string()
        );
        assert_eq!(
            short_summary(10, 0, 10),
            "All articles failed to download".red().to_string()
        );
        assert_eq!(
            short_summary(10, 8, 2),
            "8 articles downloaded successfully, 2 articles failed"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(10, 1, 9),
            "1 article downloaded successfully, 9 articles failed"
                .yellow()
                .to_string()
        );
        assert_eq!(
            short_summary(7, 6, 1),
            "6 articles downloaded successfully, 1 article failed"
                .yellow()
                .to_string()
        );
    }

    #[test]
    #[should_panic(
        expected = "initial_count must be equal to the sum of failed and successful count"
    )]
    fn test_short_summary_panics_on_invalid_input() {
        short_summary(0, 12, 43);
    }
}

#[macro_use]
extern crate lazy_static;

use async_std::stream;
use async_std::task;
use comfy_table::presets::{UTF8_FULL, UTF8_HORIZONTAL_BORDERS_ONLY};
use comfy_table::{Attribute, Cell, CellAlignment, ContentArrangement, Table};
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, warn};
use url::Url;

mod cli;
mod epub;
mod errors;
mod extractor;
/// This module is responsible for async HTTP calls for downloading
/// the HTML content and images
mod http;
mod moz_readability;

use cli::AppConfig;
use epub::generate_epubs;
use extractor::Extractor;
use http::{download_images, fetch_html};

fn main() {
    let app_config = cli::cli_init();

    if !app_config.urls().is_empty() {
        match flexi_logger::Logger::with_str("paperoni=debug")
            .log_to_file()
            .start()
        {
            Ok(_) => (),
            Err(e) => eprintln!("Unable to start logger!\n{}", e),
        }
        download(app_config);
    }
}

fn download(app_config: AppConfig) {
    let bar = ProgressBar::new(app_config.urls().len() as u64);
    let mut errors = Vec::new();
    let style = ProgressStyle::default_bar().template(
        "{spinner:.cyan} [{elapsed_precise}] {bar:40.white} {:>8} link {pos}/{len:7} {msg:.yellow/white}",
    );
    bar.set_style(style);
    bar.enable_steady_tick(500);
    let articles = task::block_on(async {
        let urls_iter = app_config.urls().iter().map(|url| fetch_html(url));
        let mut responses = stream::from_iter(urls_iter).buffered(app_config.max_conn());
        let mut articles = Vec::new();
        while let Some(fetch_result) = responses.next().await {
            match fetch_result {
                Ok((url, html)) => {
                    debug!("Extracting {}", &url);
                    let mut extractor = Extractor::from_html(&html, &url);
                    bar.set_message("Extracting...");
                    match extractor.extract_content() {
                        Ok(_) => {
                            extractor.extract_img_urls();
                            if let Err(img_errors) =
                                download_images(&mut extractor, &Url::parse(&url).unwrap(), &bar)
                                    .await
                            {
                                warn!(
                                    "{} image{} failed to download for {}",
                                    img_errors.len(),
                                    if img_errors.len() > 1 { "s" } else { "" },
                                    url
                                );
                            }
                            articles.push(extractor);
                        }
                        Err(mut e) => {
                            e.set_article_source(&url);
                            errors.push(e);
                        }
                    }
                }
                Err(e) => errors.push(e),
            }
            bar.inc(1);
        }
        articles
    });
    bar.finish_with_message("Downloaded articles");

    let mut succesful_articles_table = Table::new();
    succesful_articles_table
        .load_preset(UTF8_FULL)
        .load_preset(UTF8_HORIZONTAL_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    match generate_epubs(articles, app_config.merged(), &mut succesful_articles_table) {
        Ok(_) => (),
        Err(gen_epub_errors) => {
            errors.extend(gen_epub_errors);
        }
    };
    let successfully_downloaded_count = app_config.urls().len() - errors.len();

    println!(
        "{} articles downloaded successfully. {}",
        if successfully_downloaded_count == app_config.urls().len() {
            "All".to_string()
        } else {
            successfully_downloaded_count.to_string()
        },
        if errors.len() > 0 {
            errors.len().to_string() + " failed"
        } else {
            "".to_string()
        }
    );

    if successfully_downloaded_count > 0 {
        println!("{}", succesful_articles_table);
    }
    if !errors.is_empty() {
        println!(
            "{}Failed article downloads{}",
            Attribute::Bold,
            Attribute::NormalIntensity
        );
        let mut table_failed = Table::new();
        table_failed
            .load_preset(UTF8_HORIZONTAL_BORDERS_ONLY)
            .set_header(vec![
                Cell::new("Link").set_alignment(CellAlignment::Center),
                Cell::new("Reason").set_alignment(CellAlignment::Center),
            ])
            .set_content_arrangement(ContentArrangement::Dynamic);

        for error in errors {
            table_failed.add_row(vec![
                error
                    .article_source()
                    .clone()
                    .unwrap_or_else(|| "<unknown link>".to_string()),
                format!("{}", error.kind()),
            ]);
        }
        println!("{}", table_failed);
        std::process::exit(1);
    }
}

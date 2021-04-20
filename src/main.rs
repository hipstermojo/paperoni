#[macro_use]
extern crate lazy_static;

use async_std::stream;
use async_std::task;
use futures::stream::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
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
        download(app_config);
    }
}

fn download(app_config: AppConfig) {
    let bar = ProgressBar::new(app_config.urls().len() as u64);
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
                    // println!("Extracting");
                    let mut extractor = Extractor::from_html(&html, &url);
                    bar.set_message("Extracting...");
                    extractor.extract_content();

                    if extractor.article().is_some() {
                        extractor.extract_img_urls();
                        if let Err(img_errors) =
                            download_images(&mut extractor, &Url::parse(&url).unwrap(), &bar).await
                        {
                            eprintln!(
                                "{} image{} failed to download for {}",
                                img_errors.len(),
                                if img_errors.len() > 1 { "s" } else { "" },
                                url
                            );
                        }
                        articles.push(extractor);
                    }
                }
                Err(e) => eprintln!("{}", e),
            }
            bar.inc(1);
        }
        articles
    });
    bar.finish_with_message("Downloaded articles");
    match generate_epubs(articles, app_config.merged()) {
        Ok(_) => (),
        Err(e) => eprintln!("{}", e),
    };
}

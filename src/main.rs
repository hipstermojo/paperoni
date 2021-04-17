#[macro_use]
extern crate lazy_static;

use async_std::stream;
use async_std::task;
use futures::stream::StreamExt;
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
use http::{download_images, fetch_url};

fn main() {
    let app_config = cli::cli_init();

    if !app_config.urls().is_empty() {
        download(app_config);
    }
}

fn download(app_config: AppConfig) {
    let articles = task::block_on(async {
        let urls_iter = app_config.urls().iter().map(|url| fetch_url(url));
        let mut responses = stream::from_iter(urls_iter).buffered(app_config.max_conn());
        let mut articles = Vec::new();
        while let Some(fetch_result) = responses.next().await {
            match fetch_result {
                Ok((url, html)) => {
                    println!("Extracting");
                    let mut extractor = Extractor::from_html(&html);
                    extractor.extract_content(&url);

                    if extractor.article().is_some() {
                        extractor.extract_img_urls();

                        if let Err(img_errors) =
                            download_images(&mut extractor, &Url::parse(&url).unwrap()).await
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
        }
        articles
    });
    match generate_epubs(articles, app_config.merged()) {
        Ok(_) => (),
        Err(e) => eprintln!("{}", e),
    };
}

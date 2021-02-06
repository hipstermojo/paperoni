#[macro_use]
extern crate lazy_static;

use async_std::task;
use url::Url;

mod cli;
mod epub;
mod extractor;
/// This module is responsible for async HTTP calls for downloading
/// the HTML content and images
mod http;
mod moz_readability;

use epub::generate_epub;
use http::{download_images, fetch_url};

use extractor::Extractor;
fn main() {
    let app_config = cli::cli_init();

    if !app_config.urls().is_empty() {
        download(app_config.urls().clone());
    }
}

fn download(urls: Vec<String>) {
    let mut async_url_tasks = Vec::with_capacity(urls.len());
    for url in urls {
        async_url_tasks.push(task::spawn(async move { fetch_url(&url).await }));
    }

    task::block_on(async {
        for url_task in async_url_tasks {
            match url_task.await {
                Ok((url, html)) => {
                    println!("Extracting");
                    let mut extractor = Extractor::from_html(&html);
                    extractor.extract_content(&url);

                    if extractor.article().is_some() {
                        download_images(&mut extractor, &Url::parse(&url).unwrap())
                            .await
                            .expect("Unable to download images");
                        generate_epub(extractor);
                    }
                }
                Err(e) => println!("{}", e),
            }
        }
    })
}

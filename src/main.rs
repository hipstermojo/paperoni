#[macro_use]
extern crate lazy_static;

use std::fs::File;

use async_std::task;
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};
use url::Url;

mod cli;
mod extractor;
mod moz_readability;

use extractor::Extractor;
fn main() {
    let app = cli::cli_init();
    let arg_matches = app.get_matches();
    if let Some(vals) = arg_matches.values_of("urls") {
        let urls = vals.map(|val| val.to_string()).collect::<Vec<_>>();
        download(urls);
    }
}

type HTMLResource = (String, String);

async fn fetch_url(url: &str) -> Result<HTMLResource, Box<dyn std::error::Error>> {
    let client = surf::Client::new();
    println!("Fetching...");
    let mut res = client
        .with(surf::middleware::Redirect::default())
        .get(url)
        .send()
        .await
        .expect(&format!("Unable to fetch {}", url));
    if res.status() == 200 {
        Ok((url.to_string(), res.body_string().await?))
    } else {
        Err("Request failed to return HTTP 200".into())
    }
}

fn download(urls: Vec<String>) {
    let mut async_url_tasks = Vec::with_capacity(urls.len());
    for url in urls {
        async_url_tasks.push(task::spawn(async move { fetch_url(&url).await.unwrap() }));
    }
    task::block_on(async {
        for url_task in async_url_tasks {
            let (url, html) = url_task.await;
            println!("Extracting");
            let mut extractor = Extractor::from_html(&html);
            extractor.extract_content(&url);
            if extractor.article().is_some() {
                extractor
                    .download_images(&Url::parse(&url).unwrap())
                    .await
                    .expect("Unable to download images");
                let file_name = format!("{}.epub", extractor.metadata().title());
                let mut out_file = File::create(&file_name).unwrap();
                let mut html_buf = Vec::new();
                extractor::serialize_to_xhtml(extractor.article().unwrap(), &mut html_buf)
                    .expect("Unable to serialize to xhtml");
                let html_buf = std::str::from_utf8(&html_buf).unwrap();
                let mut epub = EpubBuilder::new(ZipLibrary::new().unwrap()).unwrap();
                if let Some(author) = extractor.metadata().byline() {
                    epub.metadata("author", author.replace("&", "&amp;"))
                        .unwrap();
                }
                epub.metadata("title", extractor.metadata().title().replace("&", "&amp;"))
                    .unwrap();
                epub.add_content(EpubContent::new("code.xhtml", html_buf.as_bytes()))
                    .unwrap();
                for img in extractor.img_urls {
                    let mut file_path = std::env::temp_dir();
                    file_path.push(&img.0);

                    let img_buf = File::open(&file_path).expect("Can't read file");
                    epub.add_resource(file_path.file_name().unwrap(), img_buf, img.1.unwrap())
                        .unwrap();
                }
                epub.generate(&mut out_file).unwrap();
                println!("Created {:?}", file_name);
            }
        }
    })
}

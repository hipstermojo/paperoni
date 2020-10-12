#[macro_use]
extern crate lazy_static;

use std::fs::File;

use async_std::{fs::create_dir, fs::remove_dir_all, task};
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};
use structopt::StructOpt;
use url::Url;

mod cli;
mod extractor;
mod moz_readability;

use extractor::Extractor;
fn main() {
    let opt = cli::Opts::from_args();
    if let Some(url) = opt.url {
        println!("Downloading single article");
        download(url)
    }
}

async fn fetch_url(url: &str) -> String {
    let client = surf::Client::new();
    println!("Fetching...");
    // TODO: Add middleware for following redirects
    client
        .get(url)
        .recv_string()
        .await
        .expect("Unable to fetch URL")
}

fn download(url: String) {
    task::block_on(async {
        let html = fetch_url(&url).await;
        let mut extractor = Extractor::from_html(&html);
        println!("Extracting");
        extractor.extract_content();
        create_dir("res/")
            .await
            .expect("Unable to create res/ output folder");
        extractor
            .download_images(&Url::parse(&url).unwrap())
            .await
            .expect("Unable to download images");
        let mut out_file = File::create("out.epub").unwrap();
        let mut html_buf = Vec::new();
        extractor
            .content
            .unwrap()
            .as_node()
            .serialize(&mut html_buf)
            .expect("Unable to serialize");
        let html_buf = std::str::from_utf8(&html_buf).unwrap();
        let mut epub = EpubBuilder::new(ZipLibrary::new().unwrap()).unwrap();
        epub.add_content(EpubContent::new("code.xhtml", html_buf.as_bytes()))
            .unwrap();
        for img in extractor.img_urls {
            let file_path = format!("{}", &img.0);

            let img_buf = File::open(file_path).expect("Can't read file");
            epub.add_resource(img.0, img_buf, img.1.unwrap()).unwrap();
        }
        epub.generate(&mut out_file).unwrap();
        println!("Cleaning up");
        remove_dir_all("res/").await.unwrap();
    })
}

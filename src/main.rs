use std::fs::File;

use async_std::{fs::create_dir, fs::remove_dir_all, task};
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};
use url::Url;

mod extractor;

use extractor::Extractor;
fn main() {
    task::block_on(async {
        let urls = vec![
            "https://saveandrun.com/posts/2020-01-24-generating-mazes-with-haskell-part-1.html",
            "https://saveandrun.com/posts/2020-04-05-querying-pacman-with-datalog.html",
            "https://blog.hipstermojo.xyz/posts/redis-orm-preface/",
            "https://vuejsdevelopers.com/2020/03/31/vue-js-form-composition-api/?utm_campaign=xl5&utm_medium=article&utm_source=vuejsnews#adding-validators",
            "https://medium.com/typeforms-engineering-blog/the-beginners-guide-to-oauth-dancing-4b8f3666de10",
            "https://dev.to/steelwolf180/full-stack-development-in-django-3768"
        ];
        let html = fetch_url(urls[4]).await;
        let mut extractor = Extractor::from_html(&html);
        println!("Extracting");
        extractor.extract_content();
        create_dir("res/")
            .await
            .expect("Unable to create res/ output folder");
        extractor
            .download_images(&Url::parse(urls[5]).unwrap())
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

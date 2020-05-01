use std::fs::File;

use async_std::task;

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
        let html = fetch_url(urls[6]).await;
        let extractor = Extractor::from_html(&html);
        println!("Extracting");
        let mut out_file = File::create("out.html").expect("Can't make file");
        extractor.extract_content(&mut out_file);
    });
}

async fn fetch_url(url: &str) -> String {
    let client = surf::Client::new();
    println!("Fetching...");
    client
        .get(url)
        .recv_string()
        .await
        .expect("Unable to fetch URL")
}

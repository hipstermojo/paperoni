use async_std::task;
use kuchiki::traits::*;

fn main() {
    task::block_on(async {
        let urls = vec![
            "https://saveandrun.com/posts/2020-01-24-generating-mazes-with-haskell-part-1.html",
            "https://saveandrun.com/posts/2020-04-05-querying-pacman-with-datalog.html",
            "https://saveandrun.com/posts/2020-01-08-working-with-git.html",
            "https://blog.hipstermojo.xyz/posts/redis-orm-preface/",
            "https://vuejsdevelopers.com/2020/03/31/vue-js-form-composition-api/?utm_campaign=xl5&utm_medium=article&utm_source=vuejsnews#adding-validators",
            "https://medium.com/typeforms-engineering-blog/the-beginners-guide-to-oauth-dancing-4b8f3666de10"
        ];
        let html = fetch_url(urls[0]).await;
        extract_content(html);
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

fn extract_content(html_str: String) {
    let document = kuchiki::parse_html().one(html_str);
    let author: Option<String> =
        document
            .select_first("meta[name='author']")
            .ok()
            .and_then(|data| {
                data.attributes
                    .borrow()
                    .get("content")
                    .map(|name| name.to_string())
            });
    let description = document
        .select_first("meta[name='description']")
        .ok()
        .and_then(|data| {
            data.attributes
                .borrow()
                .get("content")
                .map(|description| description.to_string())
        });
    let tags = document
        .select_first("meta[name='keywords']")
        .ok()
        .and_then(|data| {
            data.attributes.borrow().get("content").map(|tags| {
                tags.split(",")
                    .map(|tag_str| tag_str.trim().to_string())
                    .collect::<Vec<String>>()
            })
        });
    let title = if let Some(title_node) = document.select_first("title").ok() {
        title_node
            .as_node()
            .first_child()
            .and_then(|text_node| {
                text_node
                    .as_text()
                    .map(|text_ref| text_ref.borrow().to_string())
            })
            .unwrap_or("".to_string())
    } else {
        "".to_string()
    };
    let lang = document
        .select_first("html")
        .ok()
        .and_then(|data| {
            data.attributes
                .borrow()
                .get("lang")
                .map(|val| val.to_string())
        })
        .unwrap_or("en".to_string());
    let meta_attrs = MetaAttr::new(author, description, lang, tags, title);
    dbg!(meta_attrs);
}

#[derive(Debug)]
struct MetaAttr {
    author: Option<String>,
    description: Option<String>,
    language: String,
    tags: Option<Vec<String>>,
    title: String,
}

impl MetaAttr {
    pub fn new(
        author: Option<String>,
        description: Option<String>,
        language: String,
        tags: Option<Vec<String>>,
        title: String,
    ) -> Self {
        MetaAttr {
            author,
            description,
            language,
            tags,
            title,
        }
    }
}

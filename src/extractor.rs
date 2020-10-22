use async_std::fs::File;
use async_std::io::prelude::*;
use async_std::task;
use kuchiki::NodeRef;
use url::Url;

use super::moz_readability::Readability;

pub type ResourceInfo = (String, Option<String>);

pub struct Extractor {
    pub img_urls: Vec<ResourceInfo>,
    readability: Readability,
}

impl Extractor {
    /// Create a new instance of an HTML extractor given an HTML string
    pub fn from_html(html_str: &str) -> Self {
        Extractor {
            img_urls: Vec::new(),
            readability: Readability::new(html_str),
        }
    }

    /// Locates and extracts the HTML in a document which is determined to be
    /// the source of the content
    pub fn extract_content(&mut self, url: &str) {
        self.readability.parse(url);
    }

    /// Traverses the DOM tree of the content and retrieves the IMG URLs
    fn extract_img_urls(&mut self) {
        if let Some(content_ref) = &self.readability.article_node {
            for img_ref in content_ref.select("img").unwrap() {
                img_ref.as_node().as_element().map(|img_elem| {
                    img_elem.attributes.borrow().get("src").map(|img_url| {
                        if !img_url.is_empty() {
                            self.img_urls.push((img_url.to_string(), None))
                        }
                    })
                });
            }
        }
    }

    pub async fn download_images(&mut self, article_origin: &Url) -> async_std::io::Result<()> {
        let mut async_download_tasks = Vec::with_capacity(self.img_urls.len());
        self.extract_img_urls();
        println!("Downloading images to res/");
        for img_url in &self.img_urls {
            let img_url = img_url.0.clone();
            let abs_url = get_absolute_url(&img_url, article_origin);
            async_download_tasks.push(task::spawn(async move {
                let mut img_response = surf::get(&abs_url).await.expect("Unable to retrieve file");
                let img_content: Vec<u8> = img_response.body_bytes().await.unwrap();
                let img_mime = img_response
                    .header("Content-Type")
                    .map(|header| header.to_string());
                let img_ext = img_response
                    .header("Content-Type")
                    .and_then(map_mime_type_to_ext)
                    .unwrap();

                let img_path = format!("res/{}{}", hash_url(&abs_url), &img_ext);
                let mut img_file = File::create(&img_path)
                    .await
                    .expect("Unable to create file");
                img_file
                    .write_all(&img_content)
                    .await
                    .expect("Unable to save to file");

                (img_url, img_path, img_mime)
            }));
        }

        self.img_urls.clear();

        for async_task in async_download_tasks {
            let (img_url, img_path, img_mime) = async_task.await;
            // Update the image sources
            let img_ref = self
                .readability
                .article_node
                .as_mut()
                .expect("Unable to get mutable ref")
                .select_first(&format!("img[src='{}']", img_url))
                .expect("Image node does not exist");
            let mut img_node = img_ref.attributes.borrow_mut();
            *img_node.get_mut("src").unwrap() = img_path.clone();
            self.img_urls.push((img_path, img_mime));
        }
        Ok(())
    }

    pub fn article(&self) -> Option<&NodeRef> {
        self.readability.article_node.as_ref()
    }
}

/// Utility for hashing URLs. This is used to help store files locally with unique values
fn hash_url(url: &str) -> String {
    format!("{:x}", md5::compute(url.as_bytes()))
}

/// Handles getting the extension from a given MIME type. The extension starts with a dot
fn map_mime_type_to_ext(mime_type: &str) -> Option<String> {
    mime_type
        .split("/")
        .last()
        .map(|format| {
            if format == ("svg+xml") {
                return "svg";
            } else if format == "x-icon" {
                "ico"
            } else {
                format
            }
        })
        .map(|format| String::from(".") + format)
}

fn get_absolute_url(url: &str, request_url: &Url) -> String {
    if Url::parse(url).is_ok() {
        url.to_owned()
    } else if url.starts_with("/") {
        Url::parse(&format!(
            "{}://{}",
            request_url.scheme(),
            request_url.host_str().unwrap()
        ))
        .unwrap()
        .join(url)
        .unwrap()
        .into_string()
    } else {
        request_url.join(url).unwrap().into_string()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    const TEST_HTML: &'static str = r#"
        <!doctype html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="description" content="A sample document">
                <meta name="keywords" content="test,Rust">
                <meta name="author" content="Paperoni">                
                <title>Testing Paperoni</title>
            </head>
            <body>
                <header>
                <!-- Unimportant information -->
                    <h1>Testing Paperoni</h1>
                </header>
                <article>
                    <h1>Starting out</h1>
                    <p>Some Lorem Ipsum text here</p>
                    <p>Observe this picture</p>
                    <img src="./img.jpg" alt="Random image">
                </article>
                <footer>
                    <p>Made in HTML</p>
                </footer>
            </body>
        </html>
        "#;

    #[test]
    fn test_extract_img_urls() {
        let mut extractor = Extractor::from_html(TEST_HTML);
        extractor.extract_content("http://example.com/");
        extractor.extract_img_urls();

        assert!(extractor.img_urls.len() > 0);
        assert_eq!(
            vec![("http://example.com/img.jpg".to_string(), None)],
            extractor.img_urls
        );
    }

    #[test]
    fn test_map_mime_type_to_ext() {
        let mime_types = vec![
            "image/apng",
            "image/bmp",
            "image/gif",
            "image/x-icon",
            "image/jpeg",
            "image/png",
            "image/svg+xml",
            "image/tiff",
            "image/webp",
        ];
        let exts = mime_types
            .into_iter()
            .map(|mime_type| map_mime_type_to_ext(mime_type).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            vec![".apng", ".bmp", ".gif", ".ico", ".jpeg", ".png", ".svg", ".tiff", ".webp"],
            exts
        );
    }
}

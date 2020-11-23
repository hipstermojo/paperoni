use async_std::fs::File;
use async_std::io::prelude::*;
use async_std::task;
use kuchiki::{traits::*, NodeRef};
use url::Url;

use crate::moz_readability::{MetaData, Readability};

pub type ResourceInfo = (String, Option<String>);

pub struct Extractor {
    article: Option<NodeRef>,
    pub img_urls: Vec<ResourceInfo>,
    readability: Readability,
}

impl Extractor {
    /// Create a new instance of an HTML extractor given an HTML string
    pub fn from_html(html_str: &str) -> Self {
        Extractor {
            article: None,
            img_urls: Vec::new(),
            readability: Readability::new(html_str),
        }
    }

    /// Locates and extracts the HTML in a document which is determined to be
    /// the source of the content
    pub fn extract_content(&mut self, url: &str) {
        self.readability.parse(url);
        if let Some(article_node_ref) = &self.readability.article_node {
            let template = r#"
            <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
                <head>
                </head>
                <body>
                </body>
            </html>
            "#;
            let doc = kuchiki::parse_html().one(template);
            let body = doc.select_first("body").unwrap();
            body.as_node().append(article_node_ref.clone());
            self.article = Some(doc);
        }
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
        println!("Downloading images...");
        for img_url in &self.img_urls {
            let img_url = img_url.0.clone();
            let abs_url = get_absolute_url(&img_url, article_origin);

            async_download_tasks.push(task::spawn(async move {
                let mut img_response = surf::get(&abs_url).await.expect("Unable to retrieve file");
                let img_content: Vec<u8> = img_response.body_bytes().await.unwrap();
                let img_mime = img_response
                    .content_type()
                    .map(|mime| mime.essence().to_string());
                let img_ext = img_response
                    .content_type()
                    .map(|mime| map_mime_subtype_to_ext(mime.subtype()).to_string())
                    .unwrap();
                let mut img_path = std::env::temp_dir();
                img_path.push(format!("{}.{}", hash_url(&abs_url), &img_ext));
                let mut img_file = File::create(&img_path)
                    .await
                    .expect("Unable to create file");
                img_file
                    .write_all(&img_content)
                    .await
                    .expect("Unable to save to file");

                (
                    img_url,
                    img_path
                        .file_name()
                        .map(|os_str_name| {
                            os_str_name
                                .to_str()
                                .expect("Unable to get image file name")
                                .to_string()
                        })
                        .unwrap(),
                    img_mime,
                )
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
        self.article.as_ref()
    }

    pub fn metadata(&self) -> &MetaData {
        &self.readability.metadata
    }
}

/// Utility for hashing URLs. This is used to help store files locally with unique values
fn hash_url(url: &str) -> String {
    format!("{:x}", md5::compute(url.as_bytes()))
}

/// Handles getting the extension from a given MIME subtype.
fn map_mime_subtype_to_ext(subtype: &str) -> &str {
    if subtype == ("svg+xml") {
        return "svg";
    } else if subtype == "x-icon" {
        "ico"
    } else {
        subtype
    }
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
        let mime_subtypes = vec![
            "apng", "bmp", "gif", "x-icon", "jpeg", "png", "svg+xml", "tiff", "webp",
        ];
        let exts = mime_subtypes
            .into_iter()
            .map(|mime_type| map_mime_subtype_to_ext(mime_type))
            .collect::<Vec<_>>();
        assert_eq!(
            vec!["apng", "bmp", "gif", "ico", "jpeg", "png", "svg", "tiff", "webp"],
            exts
        );
    }
}

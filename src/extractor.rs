use async_std::fs::File;
use async_std::io::prelude::*;
use kuchiki::{traits::*, ElementData, NodeDataRef, NodeRef};
use url::Url;

pub struct Extractor {
    pub root_node: NodeRef,
    pub content: Option<NodeDataRef<ElementData>>,
    img_urls: Vec<String>,
}

impl Extractor {
    /// Create a new instance of an HTML extractor given an HTML string
    pub fn from_html(html_str: &str) -> Self {
        Extractor {
            content: None,
            img_urls: Vec::new(),
            root_node: kuchiki::parse_html().one(html_str),
        }
    }

    /// Extract the value of an attribute
    fn extract_attr_val<T: Fn(&str) -> U, U>(
        &self,
        css_selector: &str,
        attr_target: &str,
        mapper: T,
    ) -> Option<U> {
        self.root_node
            .select_first(css_selector)
            .ok()
            .and_then(|data| data.attributes.borrow().get(attr_target).map(mapper))
    }

    /// Extract the text of a DOM node given its CSS selector
    fn extract_inner_text(&self, css_selector: &str) -> Option<String> {
        let node_ref = self.root_node.select_first(css_selector).ok()?;
        extract_text_from_node(node_ref.as_node())
    }

    /// Locates and extracts the HTML in a document which is determined to be
    /// the source of the content
    pub fn extract_content(&mut self) {
        // Extract the useful parts of the head section
        let author: Option<String> =
            self.extract_attr_val("meta[name='author']", "content", |author| {
                author.to_string()
            });

        let description =
            self.extract_attr_val("meta[name='description']", "content", |description| {
                description.to_string()
            });

        let tags = self.extract_attr_val("meta[name='keywords']", "content", |tags| {
            tags.split(",")
                .map(|tag| tag.trim().to_string())
                .collect::<Vec<String>>()
        });

        let title = self.extract_inner_text("title").unwrap_or("".to_string());
        let lang = self
            .extract_attr_val("html", "lang", |lang| lang.to_string())
            .unwrap_or("en".to_string());

        let meta_attrs = MetaAttr::new(author, description, lang, tags, title);

        // Extract the article

        let article_ref = self.root_node.select_first("article").unwrap();

        for node_ref in article_ref.as_node().descendants() {
            match node_ref.data() {
                kuchiki::NodeData::Element(..) | kuchiki::NodeData::Text(..) => (),
                _ => node_ref.detach(),
            }
        }
        self.content = Some(article_ref);
    }

    /// Traverses the DOM tree of the content and retrieves the IMG URLs
    fn extract_img_urls(&mut self) {
        if let Some(content_ref) = &self.content {
            for img_ref in content_ref.as_node().select("img").unwrap() {
                img_ref.as_node().as_element().map(|img_elem| {
                    img_elem.attributes.borrow().get("src").map(|img_url| {
                        if !img_url.is_empty() {
                            self.img_urls.push(img_url.to_string())
                        }
                    })
                });
            }
        }
    }

    pub async fn download_images(&mut self, article_origin: &Url) -> async_std::io::Result<()> {
        self.extract_img_urls();
        for img_url in &self.img_urls {
            let mut img_url = img_url.clone();

            get_absolute_url(&mut img_url, article_origin);

            println!("Fetching {}", img_url);
            let mut img_response = surf::get(&img_url).await.expect("Unable to retrieve file");
            let img_content: Vec<u8> = img_response.body_bytes().await.unwrap();
            let img_ext = img_response
                .header("Content-Type")
                .and_then(map_mime_type_to_ext)
                .unwrap();
            let img_path = format!("{}{}", hash_url(&img_url), &img_ext);

            let mut img_file = File::create(&img_path).await?;
            img_file.write_all(&img_content).await?;
            println!("Image file downloaded successfully");

            let img_ref = self
                .content
                .as_mut()
                .expect("Unable to get mutable ref")
                .as_node()
                .select_first(&format!("img[src='{}']", img_url))
                .expect("Image node does not exist");
            let mut img_node = img_ref.attributes.borrow_mut();
            *img_node.get_mut("src").unwrap() = img_path;
        }
        Ok(())
    }
}
fn extract_text_from_node(node: &NodeRef) -> Option<String> {
    node.first_child()
        .map(|child_ref| child_ref.text_contents())
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

fn get_absolute_url(url: &mut String, request_url: &Url) {
    if Url::parse(url).is_ok() {
    } else if url.starts_with("/") {
        *url = Url::parse(&format!(
            "{}://{}",
            request_url.scheme(),
            request_url.host_str().unwrap()
        ))
        .unwrap()
        .join(url)
        .unwrap()
        .into_string();
    } else {
        *url = request_url.join(url).unwrap().into_string();
    }
}

#[derive(Debug)]
pub struct MetaAttr {
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
                    <img src="/img.jpg" alt="Random image">
                </article>
                <footer>
                    <p>Made in HTML</p>
                </footer>
            </body>
        </html>
        "#;

    #[test]
    fn test_extract_attr_val() {
        let extractor = Extractor::from_html(TEST_HTML);
        let ext_author =
            extractor.extract_attr_val("meta[name='author']", "content", |val| val.to_string());
        assert!(ext_author.is_some());
        assert_eq!("Paperoni", &ext_author.unwrap());
        let ext_author =
            extractor.extract_attr_val("meta[name='invalid-name']", "content", |val| {
                val.to_string()
            });
        assert!(ext_author.is_none());
        let lang_attr = extractor.extract_attr_val("html", "lang", |lang| lang.to_string());
        assert!(lang_attr.is_some());
        assert_eq!("en".to_string(), lang_attr.unwrap());
    }

    #[test]
    fn test_extract_inner_text() {
        let extractor = Extractor::from_html(TEST_HTML);
        let title_text = extractor.extract_inner_text("title");
        assert!(title_text.is_some());
        assert_eq!("Testing Paperoni".to_string(), title_text.unwrap());

        let title_text = extractor.extract_inner_text("titln");
        assert!(title_text.is_none());
    }
    #[test]
    fn test_extract_text() {
        let extractor = Extractor::from_html(TEST_HTML);
        let h1_node = extractor.root_node.select_first("h1").unwrap();
        let h1_text = extract_text_from_node(h1_node.as_node());
        assert!(h1_text.is_some());
        assert_eq!("Testing Paperoni".to_string(), h1_text.unwrap());
    }

    #[test]
    fn test_extract_content() {
        let extracted_html: String = r#"
            <article>
                <h1>Starting out</h1>
                <p>Some Lorem Ipsum text here</p>
                <p>Observe this picture</p>
                <img alt="Random image" src="./img.jpg">
            </article>
        "#
        .lines()
        .map(|line| line.trim())
        .collect();

        let mut extractor = Extractor::from_html(
            &TEST_HTML
                .lines()
                .map(|line| line.trim())
                .collect::<String>(),
        );

        extractor.extract_content();
        let mut output = Vec::new();
        assert!(extractor.content.is_some());

        extractor
            .content
            .unwrap()
            .as_node()
            .serialize(&mut output)
            .expect("Unable to serialize output HTML");
        let output = std::str::from_utf8(&output).unwrap();

        assert_eq!(extracted_html, output);
    }

    #[test]
    fn test_extract_img_urls() {
        let mut extractor = Extractor::from_html(TEST_HTML);
        extractor.extract_content();
        extractor.extract_img_urls();

        assert!(extractor.img_urls.len() > 0);
        assert_eq!(vec!["/img.jpg"], extractor.img_urls);
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

    #[test]
    fn test_get_absolute_url() {
        let mut absolute_url = "https://example.image.com/images/1.jpg".to_owned();
        let mut relative_url = "../../images/2.jpg".to_owned();
        let mut relative_from_host_url = "/images/3.jpg".to_owned();
        let host_url = Url::parse("https://example.image.com/blog/how-to-test-resolvers/").unwrap();
        get_absolute_url(&mut absolute_url, &host_url);
        assert_eq!("https://example.image.com/images/1.jpg", absolute_url);
        get_absolute_url(&mut relative_url, &host_url);
        assert_eq!("https://example.image.com/images/2.jpg", relative_url);
        relative_url = "2-1.jpg".to_owned();
        get_absolute_url(&mut relative_url, &host_url);
        assert_eq!(
            "https://example.image.com/blog/how-to-test-resolvers/2-1.jpg",
            relative_url
        );
        get_absolute_url(&mut relative_from_host_url, &host_url);
        assert_eq!(
            "https://example.image.com/images/3.jpg",
            relative_from_host_url
        );
    }
}

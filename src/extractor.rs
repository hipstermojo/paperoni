use itertools::Itertools;
use kuchiki::{traits::*, NodeRef};

use crate::errors::PaperoniError;
use crate::moz_readability::{MetaData, Readability};

/// A tuple of the url and an Option of the resource's MIME type
pub type ResourceInfo = (String, Option<String>);

pub struct Article {
    node_ref_opt: Option<NodeRef>,
    pub img_urls: Vec<ResourceInfo>,
    readability: Readability,
    pub url: String,
}

impl Article {
    /// Create a new instance of an HTML extractor given an HTML string
    pub fn from_html(html_str: &str, url: &str) -> Self {
        Self {
            node_ref_opt: None,
            img_urls: Vec::new(),
            readability: Readability::new(html_str),
            url: url.to_string(),
        }
    }

    /// Locates and extracts the HTML in a document which is determined to be
    /// the source of the content
    pub fn extract_content(&mut self) -> Result<(), PaperoniError> {
        self.readability.parse(&self.url)?;
        if let Some(article_node_ref) = &self.readability.article_node {
            let template = r#"
            <!DOCTYPE html>
            <html>
                <head>
                    <link rel="stylesheet" href="stylesheet.css" type="text/css"></link>
                </head>
                <body>
                </body>
            </html>
            "#;
            let doc = kuchiki::parse_html().one(template);
            let body = doc.select_first("body").unwrap();
            body.as_node().append(article_node_ref.clone());
            self.node_ref_opt = Some(doc);
        }
        Ok(())
    }

    /// Traverses the DOM tree of the content and retrieves the IMG URLs
    pub fn extract_img_urls(&mut self) {
        if let Some(content_ref) = &self.node_ref_opt {
            self.img_urls = content_ref
                .select("img")
                .unwrap()
                .filter_map(|img_ref| {
                    let attrs = img_ref.attributes.borrow();
                    attrs
                        .get("src")
                        .filter(|val| !(val.is_empty() || val.starts_with("data:image")))
                        .map(ToString::to_string)
                })
                .unique()
                .map(|val| (val, None))
                .collect();
        }
    }

    /// Returns the extracted article [NodeRef]. It should only be called *AFTER* calling parse
    pub fn node_ref(&self) -> &NodeRef {
        self.node_ref_opt.as_ref().expect(
            "Article node doesn't exist. This may be because the document has not been parsed",
        )
    }

    pub fn metadata(&self) -> &MetaData {
        &self.readability.metadata
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
                    <img src="data:image/png;base64,lJGWEIUQOIQWIDYVIVEDYFOUYQFWD">
                </article>
                <footer>
                    <p>Made in HTML</p>
                </footer>
            </body>
        </html>
        "#;

    #[test]
    fn test_extract_img_urls() {
        let mut article = Article::from_html(TEST_HTML, "http://example.com/");
        article
            .extract_content()
            .expect("Article extraction failed unexpectedly");
        article.extract_img_urls();

        assert!(article.img_urls.len() > 0);
        assert_eq!(
            vec![("http://example.com/img.jpg".to_string(), None)],
            article.img_urls
        );
    }
}

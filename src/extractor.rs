use std::collections::HashMap;

use kuchiki::{traits::*, NodeRef};

use crate::errors::PaperoniError;
use crate::moz_readability::{MetaData, Readability};

pub type ResourceInfo = (String, Option<String>);

lazy_static! {
    static ref ESC_SEQ_REGEX: regex::Regex = regex::Regex::new(r#"(&|<|>|'|")"#).unwrap();
}

pub struct Extractor {
    article: Option<NodeRef>,
    pub img_urls: Vec<ResourceInfo>,
    readability: Readability,
    pub url: String,
}

impl Extractor {
    /// Create a new instance of an HTML extractor given an HTML string
    pub fn from_html(html_str: &str, url: &str) -> Self {
        Extractor {
            article: None,
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
        Ok(())
    }

    /// Traverses the DOM tree of the content and retrieves the IMG URLs
    pub fn extract_img_urls(&mut self) {
        if let Some(content_ref) = &self.article {
            for img_ref in content_ref.select("img").unwrap() {
                img_ref.as_node().as_element().map(|img_elem| {
                    img_elem.attributes.borrow().get("src").map(|img_url| {
                        if !(img_url.is_empty() || img_url.starts_with("data:image")) {
                            self.img_urls.push((img_url.to_string(), None))
                        }
                    })
                });
            }
        }
    }

    /// Returns the extracted article [NodeRef]. It should only be called *AFTER* calling parse
    pub fn article(&self) -> &NodeRef {
        self.article.as_ref().expect(
            "Article node doesn't exist. This may be because the document has not been parsed",
        )
    }

    pub fn metadata(&self) -> &MetaData {
        &self.readability.metadata
    }
}

/// Serializes a NodeRef to a string that is XHTML compatible
/// The only DOM nodes serialized are Text and Element nodes
pub fn serialize_to_xhtml<W: std::io::Write>(
    node_ref: &NodeRef,
    mut w: &mut W,
) -> Result<(), PaperoniError> {
    let mut escape_map = HashMap::new();
    escape_map.insert("<", "&lt;");
    escape_map.insert(">", "&gt;");
    escape_map.insert("&", "&amp;");
    escape_map.insert("\"", "&quot;");
    escape_map.insert("'", "&apos;");
    for edge in node_ref.traverse_inclusive() {
        match edge {
            kuchiki::iter::NodeEdge::Start(n) => match n.data() {
                kuchiki::NodeData::Text(rc_text) => {
                    let text = rc_text.borrow();
                    let esc_text = ESC_SEQ_REGEX
                        .replace_all(&text, |captures: &regex::Captures| escape_map[&captures[1]]);
                    write!(&mut w, "{}", esc_text)?;
                }
                kuchiki::NodeData::Element(elem_data) => {
                    let attrs = elem_data.attributes.borrow();
                    let attrs_str = attrs
                        .map
                        .iter()
                        .map(|(k, v)| {
                            format!(
                                "{}=\"{}\"",
                                k.local,
                                ESC_SEQ_REGEX
                                    .replace_all(&v.value, |captures: &regex::Captures| {
                                        escape_map[&captures[1]]
                                    })
                            )
                        })
                        .fold("".to_string(), |acc, val| acc + " " + &val);
                    write!(&mut w, "<{}{}>", &elem_data.name.local, attrs_str)?;
                }
                _ => (),
            },
            kuchiki::iter::NodeEdge::End(n) => match n.data() {
                kuchiki::NodeData::Element(elem_data) => {
                    write!(&mut w, "</{}>", &elem_data.name.local)?;
                }
                _ => (),
            },
        }
    }
    Ok(())
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
        let mut extractor = Extractor::from_html(TEST_HTML, "http://example.com/");
        extractor
            .extract_content()
            .expect("Article extraction failed unexpectedly");
        extractor.extract_img_urls();

        assert!(extractor.img_urls.len() > 0);
        assert_eq!(
            vec![("http://example.com/img.jpg".to_string(), None)],
            extractor.img_urls
        );
    }
}

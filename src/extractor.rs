use kuchiki::{traits::*, ElementData, NodeDataRef, NodeRef};

pub struct Extractor {
    pub root_node: NodeRef,
    content: Option<NodeDataRef<ElementData>>,
}

impl Extractor {
    /// Create a new instance of an HTML extractor given an HTML string
    pub fn from_html(html_str: &str) -> Self {
        Extractor {
            content: None,
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
        dbg!(meta_attrs);

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

                _ => (),
            }
        }
    }
}
fn extract_text_from_node(node: &NodeRef) -> Option<String> {
    node.first_child()
        .map(|child_ref| child_ref.text_contents())
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
                    <img src="./img.jpg" alt="Random image">
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
    }
}

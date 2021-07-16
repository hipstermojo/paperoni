use std::collections::HashMap;
use std::fs::File;

use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use epub_builder::{EpubBuilder, EpubContent, TocElement, ZipLibrary};
use html5ever::tendril::fmt::Slice;
use indicatif::{ProgressBar, ProgressStyle};
use kuchiki::NodeRef;
use log::{debug, error, info};

use crate::{cli::AppConfig, errors::PaperoniError, extractor::Extractor};

lazy_static! {
    static ref ESC_SEQ_REGEX: regex::Regex = regex::Regex::new(r#"(&|<|>|'|")"#).unwrap();
    static ref VALID_ATTR_CHARS_REGEX: regex::Regex = regex::Regex::new(r#"[a-z0-9\-_]"#).unwrap();
}

pub fn generate_epubs(
    articles: Vec<Extractor>,
    app_config: &AppConfig,
    successful_articles_table: &mut Table,
) -> Result<(), Vec<PaperoniError>> {
    if articles.is_empty() {
        return Ok(());
    }

    let bar = if app_config.can_disable_progress_bar {
        ProgressBar::hidden()
    } else {
        let enabled_bar = ProgressBar::new(articles.len() as u64);
        let style = ProgressStyle::default_bar().template(
            "{spinner:.cyan} [{elapsed_precise}] {bar:40.white} {:>8} epub {pos}/{len:7} {msg:.green}",
        );
        enabled_bar.set_style(style);
        if !articles.is_empty() {
            enabled_bar.set_message("Generating epubs");
        }
        enabled_bar
    };

    let stylesheet = include_bytes!("./assets/writ.min.css");

    let mut errors: Vec<PaperoniError> = Vec::new();

    match app_config.merged {
        Some(ref name) => {
            successful_articles_table.set_header(vec![Cell::new("Table of Contents")
                .add_attribute(Attribute::Bold)
                .set_alignment(CellAlignment::Center)
                .fg(Color::Green)]);

            let mut epub = match EpubBuilder::new(match ZipLibrary::new() {
                Ok(zip_library) => zip_library,
                Err(err) => {
                    let mut paperoni_err: PaperoniError = err.into();
                    paperoni_err.set_article_source(name);
                    errors.push(paperoni_err);
                    return Err(errors);
                }
            }) {
                Ok(epub) => epub,
                Err(err) => {
                    let mut paperoni_err: PaperoniError = err.into();
                    paperoni_err.set_article_source(name);
                    errors.push(paperoni_err);
                    return Err(errors);
                }
            };
            debug!("Creating {:?}", name);

            if app_config.inline_toc {
                epub.inline_toc();
            }

            match epub.stylesheet(stylesheet.as_bytes()) {
                Ok(_) => (),
                Err(e) => {
                    error!("Unable to add stylesheets to epub file");
                    let mut paperoni_err: PaperoniError = e.into();
                    paperoni_err.set_article_source(name);
                    errors.push(paperoni_err);
                    return Err(errors);
                }
            }
            articles
                .iter()
                .enumerate()
                .fold(&mut epub, |epub, (idx, article)| {
                    let mut article_result = || -> Result<(), PaperoniError> {
                        let content_url = format!("article_{}.xhtml", idx);
                        let mut xhtml_buf = Vec::new();
                        let header_level_tocs =
                            get_header_level_toc_vec(&content_url, article.article());

                        serialize_to_xhtml(article.article(), &mut xhtml_buf)?;
                        let xhtml_str = std::str::from_utf8(&xhtml_buf)?;
                        let section_name = article.metadata().title();
                        let mut content = EpubContent::new(&content_url, xhtml_str.as_bytes())
                            .title(replace_escaped_characters(section_name));

                        for toc_element in header_level_tocs {
                            content = content.child(toc_element);
                        }

                        epub.metadata("title", replace_escaped_characters(name))?;
                        epub.add_content(content)?;
                        info!("Adding images for {:?}", name);
                        article.img_urls.iter().for_each(|img| {
                            // TODO: Add error handling and return errors as a vec
                            let mut file_path = std::env::temp_dir();
                            file_path.push(&img.0);

                            let img_buf = File::open(&file_path).expect("Can't read file");
                            epub.add_resource(
                                file_path.file_name().unwrap(),
                                img_buf,
                                img.1.as_ref().unwrap(),
                            )
                            .unwrap();
                        });
                        info!("Added images for {:?}", name);
                        Ok(())
                    };
                    if let Err(mut error) = article_result() {
                        error.set_article_source(&article.url);
                        errors.push(error);
                    }
                    bar.inc(1);
                    successful_articles_table.add_row(vec![article.metadata().title()]);
                    epub
                });
            let appendix = generate_appendix(articles.iter().collect());
            if let Err(err) = epub.add_content(
                EpubContent::new("appendix.xhtml", appendix.as_bytes())
                    .title(replace_escaped_characters("Article Sources")),
            ) {
                let mut paperoni_err: PaperoniError = err.into();
                paperoni_err.set_article_source(&name);
                errors.push(paperoni_err);
                return Err(errors);
            }

            let mut out_file = File::create(&name).unwrap();
            match epub.generate(&mut out_file) {
                Ok(_) => (),
                Err(err) => {
                    let mut paperoni_err: PaperoniError = err.into();
                    paperoni_err.set_article_source(&name);
                    errors.push(paperoni_err);
                    return Err(errors);
                }
            }

            bar.finish_with_message("Generated epub\n");
            debug!("Created {:?}", name);
            println!("Created {:?}", name);
        }
        None => {
            successful_articles_table
                .set_header(vec![Cell::new("Downloaded articles")
                    .add_attribute(Attribute::Bold)
                    .set_alignment(CellAlignment::Center)
                    .fg(Color::Green)])
                .set_content_arrangement(ContentArrangement::Dynamic);

            for article in &articles {
                let mut result = || -> Result<(), PaperoniError> {
                    let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
                    let file_name = format!(
                        "{}/{}.epub",
                        app_config.output_directory.as_deref().unwrap_or("."),
                        article
                            .metadata()
                            .title()
                            .replace("/", " ")
                            .replace("\\", " ")
                    );
                    debug!("Creating {:?}", file_name);
                    let mut out_file = File::create(&file_name).unwrap();
                    let mut xhtml_buf = Vec::new();
                    let header_level_tocs =
                        get_header_level_toc_vec("index.xhtml", article.article());
                    serialize_to_xhtml(article.article(), &mut xhtml_buf)
                        .expect("Unable to serialize to xhtml");
                    let xhtml_str = std::str::from_utf8(&xhtml_buf).unwrap();

                    if let Some(author) = article.metadata().byline() {
                        epub.metadata("author", replace_escaped_characters(author))?;
                    }

                    epub.stylesheet(stylesheet.as_bytes())?;

                    let title = replace_escaped_characters(article.metadata().title());
                    epub.metadata("title", &title)?;

                    let mut content =
                        EpubContent::new("index.xhtml", xhtml_str.as_bytes()).title(title);

                    for toc_element in header_level_tocs {
                        content = content.child(toc_element);
                    }

                    epub.add_content(content)?;

                    for img in &article.img_urls {
                        let mut file_path = std::env::temp_dir();
                        file_path.push(&img.0);

                        let img_buf = File::open(&file_path).expect("Can't read file");
                        epub.add_resource(
                            file_path.file_name().unwrap(),
                            img_buf,
                            img.1.as_ref().unwrap(),
                        )?;
                    }
                    let appendix = generate_appendix(vec![&article]);
                    epub.add_content(
                        EpubContent::new("appendix.xhtml", appendix.as_bytes())
                            .title(replace_escaped_characters("Article Source")),
                    )?;
                    epub.generate(&mut out_file)?;
                    bar.inc(1);

                    successful_articles_table.add_row(vec![article.metadata().title()]);

                    debug!("Created {:?}", file_name);
                    Ok(())
                };
                if let Err(mut error) = result() {
                    error.set_article_source(&article.url);
                    errors.push(error);
                }
            }
            bar.finish_with_message("Generated epubs\n");
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Replaces characters that have to be escaped before adding to the epub's metadata
fn replace_escaped_characters(value: &str) -> String {
    value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}

//TODO: The type signature of the argument should change as it requires that merged articles create an entirely new Vec of references
fn generate_appendix(articles: Vec<&Extractor>) -> String {
    let link_tags: String = articles
        .iter()
        .map(|article| {
            let article_name = if !article.metadata().title().is_empty() {
                article.metadata().title()
            } else {
                &article.url
            };
            format!(
                "<a href=\"{}\">{}</a><br></br>",
                replace_escaped_characters(&article.url),
                replace_escaped_characters(article_name)
            )
        })
        .collect();
    let template = format!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
    <head>
        <link rel="stylesheet" href="stylesheet.css" type="text/css"></link>
    </head>
    <body>
        <h2>Appendix</h2><h3>Article sources</h3>
        {}
    </body>
</html>"#,
        link_tags
    );
    template
}

/// Adds an id attribute to header elements and assigns a value based on
/// the hash of the text content. Headers with id attributes are not modified.
/// The headers here are known to have text because the grabbed article from
/// readability removes headers with no text.
fn generate_header_ids(root_node: &NodeRef) {
    let headers = root_node
        .select("h1, h2, h3, h4")
        .expect("Unable to create selector for headings");
    let headers_no_id = headers.filter(|node_data_ref| {
        let attrs = node_data_ref.attributes.borrow();
        !attrs.contains("id")
            || attrs
                .get("id")
                .map(|val| !VALID_ATTR_CHARS_REGEX.is_match(&val))
                .unwrap()
    });
    for header in headers_no_id {
        let mut attrs = header.attributes.borrow_mut();
        let text = header.text_contents();
        // The value of the id begins with an underscore because the hexadecimal
        // digest might start with a number which would make it an invalid id
        // when querying with selectors
        let value = format!("_{:x}", md5::compute(text));
        attrs.insert("id", value);
    }
}

/// Returns a vector of `TocElement` from a NodeRef used for adding to the Table of Contents for navigation
fn get_header_level_toc_vec(content_url: &str, article: &NodeRef) -> Vec<TocElement> {
    // Depth starts from 1
    const HEADER_LEVEL_MAX_DEPTH: usize = 4;
    let mut headers_vec: Vec<TocElement> = Vec::new();

    let mut header_levels = HashMap::with_capacity(HEADER_LEVEL_MAX_DEPTH);
    header_levels.insert("h1", 1);
    header_levels.insert("h2", 2);
    header_levels.insert("h3", 3);
    header_levels.insert("h4", 4);

    generate_header_ids(article);

    let headings = article
        .select("h1, h2, h3, h4")
        .expect("Unable to create selector for headings");

    // The header list will be generated using some sort of backtracking algorithm
    // There will be a stack of maximum size 4 (since it only goes to h4 now)
    let mut stack: Vec<Option<TocElement>> = std::iter::repeat(None)
        .take(HEADER_LEVEL_MAX_DEPTH)
        .collect::<_>();

    for heading in headings {
        let elem_name: &str = &heading.name.local;
        let attrs = heading.attributes.borrow();
        let id = attrs
            .get("id")
            .map(ToOwned::to_owned)
            .expect("Unable to get id value in get_header_level_toc_vec");
        let url = format!("{}#{}", content_url, id);

        let level = header_levels[elem_name];
        let index = level - 1;

        if let Some(mut existing_toc) = stack.get_mut(index).take().cloned().flatten() {
            // If a toc element already exists at that header level, consume all the toc elements
            // of a lower hierarchy e.g if the existing toc is a h2, then the h3 and h4 in the stack
            // will be consumed.
            // We collapse the children by folding from the right to the left of the stack.
            let descendants_levels = HEADER_LEVEL_MAX_DEPTH - level;
            let folded_descendants = stack
                .iter_mut()
                .rev()
                .take(descendants_levels)
                .map(|toc_elem| toc_elem.take())
                .filter(|toc_elem| toc_elem.is_some())
                .map(|toc_elem| toc_elem.unwrap())
                .reduce(|child, parent| parent.child(child));

            if let Some(child) = folded_descendants {
                existing_toc = existing_toc.child(child);
            };

            // Find the nearest ancestor to embed into.
            // If this toc_elem was a h1, then just add it to the headers_vec
            if index == 0 {
                headers_vec.push(existing_toc);
            } else {
                // Otherwise, find the nearest ancestor to add it to. If none exists, add it to the headers_vec
                let first_ancestor = stack
                    .iter_mut()
                    .take(level - 1)
                    .map(|toc_elem| toc_elem.as_mut())
                    .rfind(|toc_elem| toc_elem.is_some())
                    .flatten();

                match first_ancestor {
                    Some(ancestor_toc_elem) => {
                        *ancestor_toc_elem = ancestor_toc_elem.clone().child(existing_toc);
                    }
                    None => {
                        headers_vec.push(existing_toc);
                    }
                }
            }
        }

        if let Some(toc_elem) = stack.get_mut(index) {
            *toc_elem = Some(TocElement::new(
                url,
                replace_escaped_characters(&heading.text_contents()),
            ));
        }
    }

    let folded_stack = stack
        .into_iter()
        .rev()
        .filter(|toc_elem| toc_elem.is_some())
        .map(|opt_toc_elem| opt_toc_elem.unwrap())
        .reduce(|child, parent| parent.child(child));
    if let Some(toc_elem) = folded_stack {
        headers_vec.push(toc_elem)
    }

    headers_vec
}

/// Serializes a NodeRef to a string that is XHTML compatible
/// The only DOM nodes serialized are Text and Element nodes
fn serialize_to_xhtml<W: std::io::Write>(
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
                        .filter(|(k, _)| {
                            let attr_key: &str = &k.local;
                            attr_key.is_ascii() && VALID_ATTR_CHARS_REGEX.is_match(attr_key)
                        })
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
    use kuchiki::traits::*;

    use super::{generate_header_ids, get_header_level_toc_vec, replace_escaped_characters};

    #[test]
    fn test_replace_escaped_characters() {
        let mut value = "Lorem ipsum";
        assert_eq!(replace_escaped_characters(value), "Lorem ipsum");
        value = "Memory safe > memory unsafe";
        assert_eq!(
            replace_escaped_characters(value),
            "Memory safe &gt; memory unsafe"
        );
        value = "Author Name <author@mail.example>";
        assert_eq!(
            replace_escaped_characters(value),
            "Author Name &lt;author@mail.example&gt;"
        );
    }

    #[test]
    fn test_generate_header_ids() {
        let html_str = r#"
<!DOCTYPE html>
<html>
    <body>
        <h1>Heading 1</h1>
        <h2 id="heading-2">Heading 2</h2>
        <h2 id="heading-2-again">Heading 2 again</h2>
        <h4>Heading 4</h4>
        <h1>Heading 1 again</h1>
        <h3 class="heading">Heading 3</h3>
    </body>
</html>
        "#;
        let doc = kuchiki::parse_html().one(html_str);
        generate_header_ids(&doc);

        let mut headers = doc.select("h1, h2, h3, h4").unwrap();
        let all_headers_have_ids = headers.all(|node_data_ref| {
            let attrs = node_data_ref.attributes.borrow();
            if let Some(id) = attrs.get("id") {
                !id.trim().is_empty()
            } else {
                false
            }
        });
        assert_eq!(true, all_headers_have_ids);

        let selector = format!("h1#_{:x}", md5::compute("Heading 1"));
        assert_eq!(true, doc.select_first(&selector).is_ok());

        let selector = format!("h1#_{:x}", md5::compute("Heading 1 again"));
        assert_eq!(true, doc.select_first(&selector).is_ok());

        let selector = "h2#heading-2-again";
        assert_eq!(true, doc.select_first(selector).is_ok());
    }

    #[test]
    fn test_get_header_level_toc_vec() {
        // NOTE: Due to `TocElement` not implementing PartialEq, the tests here
        // will need to be manually written to cover for this
        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <p>Lorem ipsum</p>
            </body>
        </html>
        "#;
        let doc = kuchiki::parse_html().one(html_str);

        let toc_vec = get_header_level_toc_vec("index.xhtml", &doc);
        assert_eq!(0, toc_vec.len());

        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <h1 id="heading-1">Heading 1</h1>
                <p>Lorem ipsum</p>
                <div>
                    <h2 id="heading-2">Heading 2</h2>
                    <p>Lorem ipsum</p>
                    <p>Lorem ipsum</p>
                </div>
                <h3 id="subheading-3">Subheading 3</h2>
                <p>Lorem ipsum</p>
                <h1 id="heading-2">Second Heading 1</h2>
                <p>Lorem ipsum</p>
            </body>
        </html>
        "#;
        let doc = kuchiki::parse_html().one(html_str);

        let toc_vec = get_header_level_toc_vec("index.xhtml", &doc);
        assert_eq!(2, toc_vec.len());

        let first_h1_toc = toc_vec.first().unwrap();
        assert_eq!("Heading 1", first_h1_toc.title);
        assert_eq!(1, first_h1_toc.children.len());

        let h2_toc = first_h1_toc.children.first().unwrap();
        assert_eq!("Heading 2", h2_toc.title);
        assert_eq!(1, h2_toc.children.len());

        let h3_toc = h2_toc.children.first().unwrap();
        assert_eq!("Subheading 3", h3_toc.title);
        assert_eq!(0, h3_toc.children.len());

        let last_h1_toc = toc_vec.last().unwrap();
        assert_eq!("Second Heading 1", last_h1_toc.title);
        assert_eq!(0, last_h1_toc.children.len());

        let html_str = r#"
        <!DOCTYPE html>
        <html>
            <body>
                <h1 id="heading-1">Heading 1</h1>
                <p>Lorem ipsum</p>
                <div>
                    <h2 id="heading-2">Heading 2</h2>
                    <p>Lorem ipsum</p>
                    <p>Lorem ipsum</p>
                    <h3 id="subheading-3">Subheading 3</h2>
                    <p>Lorem ipsum</p>
                </div>
                <h2 id="heading-2">Heading 2</h2>
                <p>Lorem ipsum</p>
                <h4 id="subheading-4">Subheading 4</h4>
                <h2 id="conclusion">Conclusion</h2>
            </body>
        </html>
        "#;
        let doc = kuchiki::parse_html().one(html_str);

        let toc_vec = get_header_level_toc_vec("index.xhtml", &doc);
        assert_eq!(1, toc_vec.len());

        let h1_toc = toc_vec.first().unwrap();
        assert_eq!("Heading 1", h1_toc.title);
        assert_eq!(3, h1_toc.children.len());

        let first_h2_toc = h1_toc.children.first().unwrap();
        assert_eq!("Heading 2", first_h2_toc.title);
        assert_eq!(1, first_h2_toc.children.len());

        let h3_toc = first_h2_toc.children.first().unwrap();
        assert_eq!("Subheading 3", h3_toc.title);
        assert_eq!(0, h3_toc.children.len());
    }
}

use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    path::Path,
};

use base64::encode;
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use html5ever::{LocalName, Namespace, QualName};
use indicatif::{ProgressBar, ProgressStyle};
use kuchiki::{traits::*, NodeRef};
use log::{debug, error, info};

use crate::{
    cli::{self, AppConfig},
    errors::PaperoniError,
    extractor::Extractor,
    moz_readability::MetaData,
};

const HEAD_ELEM_NOT_FOUND: &str =
    "Unable to get <head> element to inline css. Ensure that the root node is the HTML document.";
const BASE_HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="UTF-8">
</head>
<body></body>
</html>"#;

pub fn generate_html_exports(
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
            "{spinner:.cyan} [{elapsed_precise}] {bar:40.white} {:>8} html {pos}/{len:7} {msg:.green}",
        );
        enabled_bar.set_style(style);
        if !articles.is_empty() {
            enabled_bar.set_message("Generating html files");
        }
        enabled_bar
    };

    let mut errors: Vec<PaperoniError> = Vec::new();

    match app_config.merged {
        Some(ref name) => {
            successful_articles_table.set_header(vec![Cell::new("Table of Contents")
                .add_attribute(Attribute::Bold)
                .set_alignment(CellAlignment::Center)
                .fg(Color::Green)]);

            debug!("Creating {:?}", name);

            let base_html_elem = kuchiki::parse_html().one(BASE_HTML_TEMPLATE);
            let body_elem = base_html_elem.select_first("body").unwrap();
            let base_path = Path::new(app_config.output_directory.as_deref().unwrap_or("."));
            let img_dirs_path_name = name.trim_end_matches(".html");
            let imgs_dir_path = base_path.join(img_dirs_path_name);

            if !(app_config.is_inlining_images || imgs_dir_path.exists()) {
                info!("Creating imgs dir in {:?} for {}", imgs_dir_path, name);
                if let Err(e) = std::fs::create_dir(&imgs_dir_path) {
                    error!("Unable to create imgs dir for HTML file");
                    let err: PaperoniError = e.into();
                    errors.push(err);
                    return Err(errors);
                };
            }

            for (idx, article) in articles.iter().enumerate() {
                let article_elem = article
                    .article()
                    .select_first("div[id=\"readability-page-1\"]")
                    .unwrap();

                let title = article.metadata().title();

                let mut elem_attr = article_elem.attributes.borrow_mut();
                if let Some(id_attr) = elem_attr.get_mut("id") {
                    *id_attr = format!("readability-page-{}", idx);
                }

                for (img_url, mime_type_opt) in &article.img_urls {
                    if app_config.is_inlining_images {
                        info!("Inlining images for {}", title);
                        let result = update_imgs_base64(
                            article,
                            img_url,
                            mime_type_opt.as_deref().unwrap_or("image/*"),
                        );

                        if let Err(e) = result {
                            let mut err: PaperoniError = e.into();
                            err.set_article_source(title);
                            error!("Unable to copy images to imgs dir for {}", title);
                            errors.push(err);
                        }

                        info!("Completed inlining images for {}", title);
                    } else {
                        info!("Copying images to imgs dir for {}", title);
                        let result = update_img_urls(article, &imgs_dir_path).map_err(|e| {
                            let mut err: PaperoniError = e.into();
                            err.set_article_source(title);
                            err
                        });
                        if let Err(e) = result {
                            error!("Unable to copy images to imgs dir for {}", title);
                            errors.push(e);
                        } else {
                            info!("Successfully copied images to imgs dir for {}", title);
                        }
                    }
                }
                bar.inc(1);
                successful_articles_table.add_row(vec![title]);
                body_elem.as_node().append(article_elem.as_node().clone());
                debug!("Added {} to the export HTML file", title);
            }

            insert_title_elem(&base_html_elem, name);
            insert_appendix(
                &base_html_elem,
                articles
                    .iter()
                    .map(|article| (article.metadata(), article.url.as_str()))
                    .collect(),
            );
            inline_css(&base_html_elem, app_config);

            info!("Added title, footer and inlined styles for {}", name);

            info!("Creating export HTML file: {}", name);
            if let Err(mut err) = File::create(name)
                .and_then(|mut out_file| base_html_elem.serialize(&mut out_file))
                .map_err(|e| -> PaperoniError { e.into() })
            {
                error!("Failed to serialize articles to file: {}", name);
                err.set_article_source(&name);
                errors.push(err);
                bar.finish_with_message("html generation failed");
                return Err(errors);
            };

            bar.finish_with_message("Generated html file\n");
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

            let mut file_names: HashSet<String> = HashSet::new();

            for article in &articles {
                let mut file_name = format!(
                    "{}/{}.html",
                    app_config.output_directory.as_deref().unwrap_or("."),
                    article
                        .metadata()
                        .title()
                        .replace("/", " ")
                        .replace("\\", " ")
                );

                if file_names.contains(&file_name) {
                    info!("Article name {:?} already exists", file_name);
                    file_name = format!(
                        "{}/{}_{}.html",
                        app_config.output_directory.as_deref().unwrap_or("."),
                        article
                            .metadata()
                            .title()
                            .replace("/", " ")
                            .replace("\\", " "),
                        file_names.len()
                    );
                    info!("Renamed to {:?}", file_name);
                }
                file_names.insert(file_name.clone());

                debug!("Creating {:?}", file_name);
                let export_article = || -> Result<(), PaperoniError> {
                    let mut out_file = File::create(&file_name)?;

                    if app_config.is_inlining_images {
                        for (img_url, mime_type_opt) in &article.img_urls {
                            update_imgs_base64(
                                article,
                                img_url,
                                mime_type_opt.as_deref().unwrap_or("image/*"),
                            )?
                        }
                    } else {
                        let base_path =
                            Path::new(app_config.output_directory.as_deref().unwrap_or("."));
                        let imgs_dir_name = article.metadata().title();

                        if !base_path.join(imgs_dir_name).exists() {
                            std::fs::create_dir(base_path.join(imgs_dir_name))?;
                        }

                        let imgs_dir_path = base_path.join(imgs_dir_name);
                        update_img_urls(article, &imgs_dir_path)?;
                    }

                    let utf8_encoding =
                        NodeRef::new_element(create_qualname("meta"), BTreeMap::new());
                    if let Some(elem_node) = utf8_encoding.as_element() {
                        let mut elem_attrs = elem_node.attributes.borrow_mut();
                        elem_attrs.insert("charset", "UTF-8".into());
                    }

                    if let Ok(head_elem) = article.article().select_first("head") {
                        let head_elem_node = head_elem.as_node();
                        head_elem_node.append(utf8_encoding);
                    };

                    insert_title_elem(article.article(), article.metadata().title());
                    insert_appendix(article.article(), vec![(article.metadata(), &article.url)]);
                    inline_css(article.article(), app_config);

                    article.article().serialize(&mut out_file)?;
                    Ok(())
                };

                if let Err(mut err) = export_article() {
                    err.set_article_source(&article.url);
                    errors.push(err);
                }
                debug!("Created {:?}", file_name);

                bar.inc(1);
                successful_articles_table.add_row(vec![article.metadata().title()]);
            }
            bar.finish_with_message("Generated HTML files\n");
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn create_qualname(name: &str) -> QualName {
    QualName::new(
        None,
        Namespace::from("http://www.w3.org/1999/xhtml"),
        LocalName::from(name),
    )
}

/// Updates the src attribute of `<img>` elements with a base64 encoded string of the image data
fn update_imgs_base64(
    article: &Extractor,
    img_url: &str,
    mime_type: &str,
) -> Result<(), std::io::Error> {
    let temp_dir = std::env::temp_dir();
    let img_path = temp_dir.join(img_url);
    let img_bytes = std::fs::read(img_path)?;
    let img_base64_str = format!("data:image:{};base64,{}", mime_type, encode(img_bytes));

    let img_elems = article
        .article()
        .select(&format!("img[src=\"{}\"]", img_url))
        .unwrap();
    for img_elem in img_elems {
        let mut img_attr = img_elem.attributes.borrow_mut();
        if let Some(src_attr) = img_attr.get_mut("src") {
            *src_attr = img_base64_str.clone();
        }
    }
    Ok(())
}

/// Updates the src attribute of `<img>` elements to the new `imgs_dir_path` and copies the image to the new file location
fn update_img_urls(article: &Extractor, imgs_dir_path: &Path) -> Result<(), std::io::Error> {
    let temp_dir = std::env::temp_dir();
    for (img_url, _) in &article.img_urls {
        let (from, to) = (temp_dir.join(img_url), imgs_dir_path.join(img_url));
        info!("Copying {:?} to {:?}", from, to);
        fs::copy(from, to)?;
        let img_elems = article
            .article()
            .select(&format!("img[src=\"{}\"]", img_url))
            .unwrap();
        for img_elem in img_elems {
            let mut img_attr = img_elem.attributes.borrow_mut();
            if let Some(src_attr) = img_attr.get_mut("src") {
                *src_attr = imgs_dir_path.join(img_url).to_str().unwrap().into();
            }
        }
    }
    Ok(())
}

/// Creates a `<title>` element in an HTML document with the value set to the article's title
fn insert_title_elem(root_node: &NodeRef, title: &str) {
    let title_content = NodeRef::new_text(title);
    let title_elem = NodeRef::new_element(create_qualname("title"), BTreeMap::new());
    title_elem.append(title_content);
    match root_node.select_first("head") {
        Ok(head_elem) => {
            head_elem.as_node().append(title_elem);
        }
        Err(_) => {
            debug!("{}", HEAD_ELEM_NOT_FOUND);
            let html_elem = root_node.select_first("html").unwrap();
            let head_elem = NodeRef::new_element(create_qualname("head"), BTreeMap::new());
            head_elem.append(title_elem);
            html_elem.as_node().prepend(head_elem);
        }
    }
}

/// Creates the appendix in an HTML document where article sources are added in a `<footer>` element
fn insert_appendix(root_node: &NodeRef, article_links: Vec<(&MetaData, &str)>) {
    let link_tags: String = article_links
        .iter()
        .map(|(meta_data, url)| {
            let article_name = if !meta_data.title().is_empty() {
                meta_data.title()
            } else {
                url
            };
            format!("<a href=\"{}\">{}</a><br></br>", url, article_name)
        })
        .collect();
    let footer_inner_html = format!("<h2>Appendix</h2><h2>Article sources</h3>{}", link_tags);
    let footer_elem =
        kuchiki::parse_fragment(create_qualname("footer"), Vec::new()).one(footer_inner_html);
    root_node.append(footer_elem);
}

/// Inlines the CSS stylesheets into the HTML article node
fn inline_css(root_node: &NodeRef, app_config: &AppConfig) {
    let body_stylesheet = include_str!("./assets/body.min.css");
    let header_stylesheet = include_str!("./assets/headers.min.css");
    let mut css_str = String::new();
    match app_config.css_config {
        cli::CSSConfig::NoHeaders => {
            css_str.push_str(body_stylesheet);
        }
        cli::CSSConfig::All => {
            css_str.push_str(body_stylesheet);
            css_str.push_str(header_stylesheet);
        }
        cli::CSSConfig::None => {
            return;
        }
    }
    let css_html_str = format!("<style>{}</style>", css_str);
    let style_container =
        kuchiki::parse_fragment(create_qualname("div"), Vec::new()).one(css_html_str);
    let style_elem = style_container.select_first("style").unwrap();
    match root_node.select_first("head") {
        Ok(head_elem) => {
            head_elem.as_node().prepend(style_elem.as_node().to_owned());
        }
        Err(_) => {
            debug!("{}", HEAD_ELEM_NOT_FOUND);
            let html_elem = root_node.select_first("html").unwrap();
            let head_elem = NodeRef::new_element(create_qualname("head"), BTreeMap::new());
            head_elem.prepend(style_elem.as_node().to_owned());
            html_elem.as_node().prepend(head_elem);
        }
    }

    // Remove the <link> of the stylesheet since styles are now inlined
    if let Ok(style_link_elem) = root_node.select_first("link[href=\"stylesheet.css\"]") {
        style_link_elem.as_node().detach();
    };
}

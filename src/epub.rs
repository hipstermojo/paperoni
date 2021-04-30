use std::fs::File;

use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};
use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info};

use crate::{
    cli::AppConfig,
    errors::PaperoniError,
    extractor::{self, Extractor},
};

pub fn generate_epubs(
    articles: Vec<Extractor>,
    app_config: &AppConfig,
    successful_articles_table: &mut Table,
) -> Result<(), Vec<PaperoniError>> {
    let bar = if app_config.can_disable_progress_bar() {
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

    let mut errors: Vec<PaperoniError> = Vec::new();

    match app_config.merged() {
        Some(name) => {
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
            epub.inline_toc();
            articles
                .iter()
                .enumerate()
                .fold(&mut epub, |epub, (idx, article)| {
                    let mut article_result = || -> Result<(), PaperoniError> {
                        let mut html_buf = Vec::new();
                        extractor::serialize_to_xhtml(article.article(), &mut html_buf)?;
                        let html_str = std::str::from_utf8(&html_buf)?;
                        epub.metadata("title", replace_metadata_value(name))?;
                        let section_name = article.metadata().title();
                        epub.add_content(
                            EpubContent::new(format!("article_{}.xhtml", idx), html_str.as_bytes())
                                .title(replace_metadata_value(section_name)),
                        )?;
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
                    .title(replace_metadata_value("Article Sources")),
            ) {
                let mut paperoni_err: PaperoniError = err.into();
                paperoni_err.set_article_source(name);
                errors.push(paperoni_err);
                return Err(errors);
            }

            let mut out_file = File::create(&name).unwrap();
            match epub.generate(&mut out_file) {
                Ok(_) => (),
                Err(err) => {
                    let mut paperoni_err: PaperoniError = err.into();
                    paperoni_err.set_article_source(name);
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
                        "{}.epub",
                        article
                            .metadata()
                            .title()
                            .replace("/", " ")
                            .replace("\\", " ")
                    );
                    debug!("Creating {:?}", file_name);
                    let mut out_file = File::create(&file_name).unwrap();
                    let mut html_buf = Vec::new();
                    extractor::serialize_to_xhtml(article.article(), &mut html_buf)
                        .expect("Unable to serialize to xhtml");
                    let html_str = std::str::from_utf8(&html_buf).unwrap();
                    if let Some(author) = article.metadata().byline() {
                        epub.metadata("author", replace_metadata_value(author))?;
                    }
                    epub.metadata("title", replace_metadata_value(article.metadata().title()))?;
                    epub.add_content(EpubContent::new("index.xhtml", html_str.as_bytes()))?;
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
                            .title(replace_metadata_value("Article Source")),
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
fn replace_metadata_value(value: &str) -> String {
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
                replace_metadata_value(&article.url),
                replace_metadata_value(article_name)
            )
        })
        .collect();
    let template = format!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
    <head>
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

#[cfg(test)]
mod test {
    use super::replace_metadata_value;

    #[test]
    fn test_replace_metadata_value() {
        let mut value = "Lorem ipsum";
        assert_eq!(replace_metadata_value(value), "Lorem ipsum");
        value = "Memory safe > memory unsafe";
        assert_eq!(
            replace_metadata_value(value),
            "Memory safe &gt; memory unsafe"
        );
        value = "Author Name <author@mail.example>";
        assert_eq!(
            replace_metadata_value(value),
            "Author Name &lt;author@mail.example&gt;"
        );
    }
}

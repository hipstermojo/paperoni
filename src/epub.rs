use std::fs::File;

use comfy_table::presets::{UTF8_FULL, UTF8_HORIZONTAL_BORDERS_ONLY};
use comfy_table::{Attribute, Cell, CellAlignment, Color, ContentArrangement, Table};
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};
use indicatif::{ProgressBar, ProgressStyle};

use crate::{
    errors::PaperoniError,
    extractor::{self, Extractor},
};

pub fn generate_epubs(
    articles: Vec<Extractor>,
    merged: Option<&String>,
) -> Result<(), PaperoniError> {
    let bar = ProgressBar::new(articles.len() as u64);
    let style = ProgressStyle::default_bar().template(
        "{spinner:.cyan} [{elapsed_precise}] {bar:40.white} {:>8} epub {pos}/{len:7} {msg:.green}",
    );
    bar.set_style(style);
    bar.set_message("Generating epubs");
    let mut base_table = Table::new();
    base_table
        .load_preset(UTF8_FULL)
        .load_preset(UTF8_HORIZONTAL_BORDERS_ONLY)
        .set_content_arrangement(ContentArrangement::Dynamic);
    match merged {
        Some(name) => {
            base_table.set_header(vec![Cell::new("Table of Contents")
                .add_attribute(Attribute::Bold)
                .set_alignment(CellAlignment::Center)
                .fg(Color::Green)]);
            let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
            epub.inline_toc();
            epub = articles
                .iter()
                .enumerate()
                .fold(epub, |mut epub, (idx, article)| {
                    let mut html_buf = Vec::new();
                    extractor::serialize_to_xhtml(article.article().unwrap(), &mut html_buf)
                        .expect("Unable to serialize to xhtml");
                    let html_str = std::str::from_utf8(&html_buf).unwrap();
                    epub.metadata("title", replace_metadata_value(name))
                        .unwrap();
                    let section_name = article.metadata().title();
                    epub.add_content(
                        EpubContent::new(format!("article_{}.xhtml", idx), html_str.as_bytes())
                            .title(replace_metadata_value(section_name)),
                    )
                    .unwrap();

                    article.img_urls.iter().for_each(|img| {
                        // TODO: Add error handling
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
                    bar.inc(1);
                    base_table.add_row(vec![article.metadata().title()]);
                    epub
                });
            let mut out_file = File::create(&name).unwrap();
            epub.generate(&mut out_file)?;
            bar.finish_with_message("Generated epub\n");
            println!("Created {:?}", name);
        }
        None => {
            base_table
                .set_header(vec![Cell::new("Downloaded articles")
                    .add_attribute(Attribute::Bold)
                    .set_alignment(CellAlignment::Center)
                    .fg(Color::Green)])
                .set_content_arrangement(ContentArrangement::Dynamic);

            for article in articles {
                let mut epub = EpubBuilder::new(ZipLibrary::new()?)?;
                let file_name = format!(
                    "{}.epub",
                    article
                        .metadata()
                        .title()
                        .replace("/", " ")
                        .replace("\\", " ")
                );
                let mut out_file = File::create(&file_name).unwrap();
                let mut html_buf = Vec::new();
                extractor::serialize_to_xhtml(article.article().unwrap(), &mut html_buf)
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
                epub.generate(&mut out_file)?;
                bar.inc(1);

                base_table.add_row(vec![article.metadata().title()]);

                // println!("Created {:?}", file_name);
            }
            bar.finish_with_message("Generated epubs\n");
        }
    }
    println!("{}", base_table);
    Ok(())
}

/// Replaces characters that have to be escaped before adding to the epub's metadata
fn replace_metadata_value(value: &str) -> String {
    value
        .replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
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

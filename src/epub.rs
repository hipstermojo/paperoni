use std::fs::File;

use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

use crate::{
    errors::PaperoniError,
    extractor::{self, Extractor},
};

pub fn generate_epubs(
    articles: Vec<Extractor>,
    merged: Option<&String>,
) -> Result<(), PaperoniError> {
    match merged {
        Some(name) => {
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
                    epub
                });
            let mut out_file = File::create(&name).unwrap();
            epub.generate(&mut out_file)?;
            println!("Created {:?}", name);
        }
        None => {
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
                for img in article.img_urls {
                    let mut file_path = std::env::temp_dir();
                    file_path.push(&img.0);

                    let img_buf = File::open(&file_path).expect("Can't read file");
                    epub.add_resource(file_path.file_name().unwrap(), img_buf, img.1.unwrap())?;
                }
                epub.generate(&mut out_file)?;
                println!("Created {:?}", file_name);
            }
        }
    }
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

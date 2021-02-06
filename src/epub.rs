use std::fs::File;

use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

use crate::extractor::{self, Extractor};

pub fn generate_epub(extractor: Extractor) {
    let file_name = format!(
        "{}.epub",
        extractor
            .metadata()
            .title()
            .replace("/", " ")
            .replace("\\", " ")
    );
    let mut out_file = File::create(&file_name).unwrap();
    let mut html_buf = Vec::new();
    extractor::serialize_to_xhtml(extractor.article().unwrap(), &mut html_buf)
        .expect("Unable to serialize to xhtml");
    let html_buf = std::str::from_utf8(&html_buf).unwrap();
    let mut epub = EpubBuilder::new(ZipLibrary::new().unwrap()).unwrap();
    if let Some(author) = extractor.metadata().byline() {
        epub.metadata("author", replace_metadata_value(author))
            .unwrap();
    }
    epub.metadata(
        "title",
        replace_metadata_value(extractor.metadata().title()),
    )
    .unwrap();
    epub.add_content(EpubContent::new("index.xhtml", html_buf.as_bytes()))
        .unwrap();
    for img in extractor.img_urls {
        let mut file_path = std::env::temp_dir();
        file_path.push(&img.0);

        let img_buf = File::open(&file_path).expect("Can't read file");
        epub.add_resource(file_path.file_name().unwrap(), img_buf, img.1.unwrap())
            .unwrap();
    }
    epub.generate(&mut out_file).unwrap();
    println!("Created {:?}", file_name);
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

use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "paperoni")]
/// Paperoni is an article downloader.
///
/// It takes a url and downloads the article content from it and
/// saves it to an epub.
pub struct Opts {
    // #[structopt(conflicts_with("links"))]
    /// Url of a web article
    pub url: Option<String>,
}

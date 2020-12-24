use clap::{App, AppSettings, Arg};

pub fn cli_init() -> App<'static, 'static> {
    App::new("paperoni")
        .settings(&[
            AppSettings::ArgRequiredElseHelp,
            AppSettings::UnifiedHelpMessage,
        ])
        .version("0.2.1-alpha1")
        .about(
            "
Paperoni is an article downloader.
It takes a url and downloads the article content from it and saves it to an epub.
        ",
        )
        .arg(
            Arg::with_name("urls")
                .help("Urls of web articles")
                .multiple(true),
        )
}

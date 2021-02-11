use std::{fs::File, io::Read};

use clap::{App, AppSettings, Arg};

pub fn cli_init() -> AppConfig {
    let app = App::new("paperoni")
        .settings(&[
            AppSettings::ArgRequiredElseHelp,
            AppSettings::UnifiedHelpMessage,
        ])
        .version("0.2.2-alpha1")
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
        .arg(
            Arg::with_name("file")
                .short("f")
                .long("file")
                .help("Input file containing links")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("output_name")
                .long("merge")
                .help("Merge multiple articles into a single epub")
                .long_help("Merge multiple articles into a single epub that will be given the name provided")
                .takes_value(true),
        );
    let arg_matches = app.get_matches();
    let mut urls: Vec<String> = match arg_matches.value_of("file") {
        Some(file_name) => {
            if let Ok(mut file) = File::open(file_name) {
                let mut content = String::new();
                match file.read_to_string(&mut content) {
                    Ok(_) => content
                        .lines()
                        .filter(|line| !line.is_empty())
                        .map(|line| line.to_owned())
                        .collect(),
                    Err(_) => vec![],
                }
            } else {
                println!("Unable to open file: {}", file_name);
                vec![]
            }
        }
        None => vec![],
    };

    if let Some(vals) = arg_matches.values_of("urls") {
        urls.extend(
            vals.filter(|val| !val.is_empty())
                .map(|val| val.to_string()),
        );
    }

    let mut app_config = AppConfig::new();
    app_config.set_urls(urls);
    if let Some(name) = arg_matches.value_of("output_name") {
        let file_name = if name.ends_with(".epub") && name.len() > 5 {
            name.to_owned()
        } else {
            name.to_owned() + ".epub"
        };
        app_config.set_merged(file_name);
    }
    app_config
}

pub struct AppConfig {
    urls: Vec<String>,
    max_conn: usize,
    merged: Option<String>,
}

impl AppConfig {
    fn new() -> Self {
        Self {
            urls: vec![],
            max_conn: 8,
            merged: None,
        }
    }

    fn set_urls(&mut self, urls: Vec<String>) {
        self.urls.extend(urls);
    }

    fn set_merged(&mut self, name: String) {
        self.merged = Some(name);
    }

    pub fn urls(&self) -> &Vec<String> {
        &self.urls
    }
    pub fn max_conn(&self) -> usize {
        self.max_conn
    }

    pub fn merged(&self) -> Option<&String> {
        self.merged.as_ref()
    }
}

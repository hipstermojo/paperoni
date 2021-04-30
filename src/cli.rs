use std::{fs::File, io::Read, path::Path};

use chrono::{DateTime, Local};
use clap::{App, AppSettings, Arg};
use flexi_logger::LevelFilter as LogLevel;

use crate::logs::init_logger;

pub fn cli_init() -> AppConfig {
    let app = App::new("paperoni")
        .settings(&[
            AppSettings::ArgRequiredElseHelp,
            AppSettings::UnifiedHelpMessage,
        ])
        .version(clap::crate_version!())
        .about(
            "
Paperoni is an article downloader.
It takes a url, downloads the article content from it and saves it to an epub.
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
        ).arg(
            Arg::with_name("max_conn")
                .long("max_conn")
                .help("The maximum number of concurrent HTTP connections when downloading articles. Default is 8")
                .long_help("The maximum number of concurrent HTTP connections when downloading articles. Default is 8.\nNOTE: It is advised to use as few connections as needed i.e between 1 and 50. Using more connections can end up overloading your network card with too many concurrent requests.")
                .takes_value(true))
        .arg(
            Arg::with_name("verbosity")
                .short("v")
                .multiple(true)
                .help("Enables logging of events and set the verbosity level. Use -h to read on its usage")
                .long_help(
"This takes upto 4 levels of verbosity in the following order.
 - Error (-v)
 - Warn (-vv)
 - Info (-vvv)
 - Debug (-vvvv)
 When this flag is passed, it disables the progress bars and logs to stderr.
 If you would like to send the logs to a file (and enable progress bars), pass the log-to-file flag."
                )
                .takes_value(false))
        .arg(
            Arg::with_name("log-to-file")
                .long("log-to-file")
                .help("Enables logging of events to a file located in .paperoni/logs with a default log level of debug. Use -v to specify the logging level")
                .takes_value(false));
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

    let max_conn = arg_matches
        .value_of("max_conn")
        .map(|conn_str| conn_str.parse::<usize>().ok())
        .flatten()
        .map(|max| if max > 0 { max } else { 1 })
        .unwrap_or(8);

    let mut app_config = AppConfig::new(max_conn);
    app_config.set_urls(urls);

    if let Some(name) = arg_matches.value_of("output_name") {
        let file_path = Path::new(name);
        if !file_path.is_file() {
            eprintln!("{:?} is not a vaild file", name);
            std::process::exit(1);
        }

        let file_name = if name.ends_with(".epub") && name.len() > 5 {
            name.to_owned()
        } else {
            name.to_owned() + ".epub"
        };
        app_config.merged = Some(file_name);
    }

    if arg_matches.is_present("verbosity") {
        if !arg_matches.is_present("log-to-file") {
            app_config.can_disable_progress_bar = true;
        }
        let log_levels: [LogLevel; 5] = [
            LogLevel::Off,
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
        ];
        let level = arg_matches.occurrences_of("verbosity").clamp(0, 4) as usize;
        app_config.log_level = log_levels[level];
    }
    if arg_matches.is_present("log-to-file") {
        app_config.log_level = LogLevel::Debug;
        app_config.is_logging_to_file = true;
    }

    init_logger(&app_config);

    app_config
}

pub struct AppConfig {
    urls: Vec<String>,
    max_conn: usize,
    merged: Option<String>,
    log_level: LogLevel,
    can_disable_progress_bar: bool,
    start_time: DateTime<Local>,
    is_logging_to_file: bool,
}

impl AppConfig {
    fn new(max_conn: usize) -> Self {
        Self {
            urls: vec![],
            max_conn,
            merged: None,
            log_level: LogLevel::Off,
            can_disable_progress_bar: false,
            start_time: Local::now(),
            is_logging_to_file: false,
        }
    }

    fn set_urls(&mut self, urls: Vec<String>) {
        self.urls.extend(urls);
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

    pub fn log_level(&self) -> LogLevel {
        self.log_level
    }

    pub fn can_disable_progress_bar(&self) -> bool {
        self.can_disable_progress_bar
    }

    pub fn start_time(&self) -> &DateTime<Local> {
        &self.start_time
    }

    pub fn is_logging_to_file(&self) -> bool {
        self.is_logging_to_file
    }
}

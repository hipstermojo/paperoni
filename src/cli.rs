use std::{
    collections::HashSet,
    num::{NonZeroUsize, ParseIntError},
    path::Path,
};

use chrono::{DateTime, Local};
use clap::{App, AppSettings, Arg, ArgMatches};
use flexi_logger::{FlexiLoggerError, LevelFilter as LogLevel};
use std::fs;
use thiserror::Error;

const DEFAULT_MAX_CONN: usize = 8;

#[derive(derive_builder::Builder)]
pub struct AppConfig {
    /// Urls for store in epub
    pub urls: Vec<String>,
    pub max_conn: usize,
    /// Path to file of multiple articles into a single epub
    pub merged: Option<String>,
    pub output_directory: Option<String>,
    pub log_level: LogLevel,
    pub can_disable_progress_bar: bool,
    pub start_time: DateTime<Local>,
    pub is_logging_to_file: bool,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to open file with urls: {0}")]
    UrlFileError(#[from] std::io::Error),
    #[error("Failed to parse max connection value: {0}")]
    InvalidMaxConnectionCount(#[from] ParseIntError),
    #[error("No urls for parse")]
    NoUrls,
    #[error("No urls for parse")]
    AppBuildError(#[from] AppConfigBuilderError),
    #[error("Invalid output path name for merged epubs: {0}")]
    InvalidOutputPath(String),
    #[error("Log error: {0}")]
    LogDirectoryError(String),
    #[error(transparent)]
    LogError(#[from] FlexiLoggerError),
    #[error("Wrong output directory")]
    WrongOutputDirectory,
    #[error("Output directory not exists")]
    OutputDirectoryNotExists,
}

impl AppConfig {
    pub fn init_with_cli() -> Result<AppConfig, Error> {
        let app = App::new("paperoni")
        .settings(&[
            AppSettings::ArgRequiredElseHelp,
            AppSettings::UnifiedHelpMessage,
        ])
        .version(clap::crate_version!())
        .about(
            "Paperoni is a CLI tool made in Rust for downloading web articles as EPUBs",
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
            Arg::with_name("output_directory")
                .long("output_directory")
                .short("o")
                .help("Directory for store output epub documents")
                .conflicts_with("output_name")
                .long_help("Directory for saving epub documents")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("output_name")
                .long("merge")
                .help("Merge multiple articles into a single epub")
                .long_help("Merge multiple articles into a single epub that will be given the name provided")
                .conflicts_with("output_directory")
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
                .help("Enables logging of events and set the verbosity level. Use --help to read on its usage")
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

        Self::try_from(app.get_matches())
    }

    fn init_merge_file(self) -> Result<Self, Error> {
        self.merged
            .as_deref()
            .map(fs::File::create)
            .transpose()
            .err()
            .map(|err| Err(Error::InvalidOutputPath(err.to_string())))
            .unwrap_or(Ok(self))
    }

    fn init_logger(self) -> Result<Self, Error> {
        use directories::UserDirs;
        use flexi_logger::LogSpecBuilder;

        match UserDirs::new() {
            Some(user_dirs) => {
                let home_dir = user_dirs.home_dir();
                let paperoni_dir = home_dir.join(".paperoni");
                let log_dir = paperoni_dir.join("logs");

                let log_spec = LogSpecBuilder::new()
                    .module("paperoni", self.log_level)
                    .build();
                let formatted_timestamp = self.start_time.format("%Y-%m-%d_%H-%M-%S");
                let mut logger = flexi_logger::Logger::with(log_spec);

                if self.is_logging_to_file && (!paperoni_dir.is_dir() || !log_dir.is_dir()) {
                    if let Err(e) = fs::create_dir_all(&log_dir) {
                        return Err(Error::LogDirectoryError(format!("Unable to create paperoni directories on home directory for logging purposes\n{}",e)));
                    }
                }
                if self.is_logging_to_file {
                    logger = logger
                        .directory(log_dir)
                        .discriminant(formatted_timestamp.to_string())
                        .suppress_timestamp()
                        .log_to_file();
                }
                logger.start()?;
                Ok(self)
            }
            None => Err(Error::LogDirectoryError(
                "Unable to get user directories for logging purposes".to_string(),
            )),
        }
    }
}

use std::convert::TryFrom;

impl<'a> TryFrom<ArgMatches<'a>> for AppConfig {
    type Error = Error;

    fn try_from(arg_matches: ArgMatches<'a>) -> Result<Self, Self::Error> {
        AppConfigBuilder::default()
            .urls({
                let url_filter = |url: &str| {
                    let url = url.trim();
                    if !url.is_empty() {
                        Some(url.to_owned())
                    } else {
                        None
                    }
                };
                match (
                    arg_matches
                        .values_of("urls")
                        .and_then(|urls| urls.map(url_filter).collect::<Option<HashSet<_>>>()),
                    arg_matches
                        .value_of("file")
                        .map(fs::read_to_string)
                        .transpose()?
                        .and_then(|content| {
                            content
                                .lines()
                                .map(url_filter)
                                .collect::<Option<HashSet<_>>>()
                        }),
                ) {
                    (Some(direct_urls), Some(file_urls)) => Ok(direct_urls
                        .union(&file_urls)
                        .map(ToOwned::to_owned)
                        .collect::<Vec<_>>()),
                    (Some(urls), None) | (None, Some(urls)) => Ok(urls.into_iter().collect()),
                    (None, None) => Err(Error::NoUrls),
                }
            }?)
            .max_conn(match arg_matches.value_of("max_conn") {
                Some(max_conn) => max_conn.parse::<NonZeroUsize>()?.get(),
                None => DEFAULT_MAX_CONN,
            })
            .merged(arg_matches.value_of("output_name").map(ToOwned::to_owned))
            .can_disable_progress_bar(
                arg_matches.is_present("verbosity") && !arg_matches.is_present("log-to-file"),
            )
            .log_level(match arg_matches.occurrences_of("verbosity") {
                0 => LogLevel::Off,
                1 => LogLevel::Error,
                2 => LogLevel::Warn,
                3 => LogLevel::Info,
                4..=u64::MAX => LogLevel::Debug,
            })
            .is_logging_to_file(arg_matches.is_present("log-to_file"))
            .output_directory(
                arg_matches
                    .value_of("output_directory")
                    .map(|output_directory| {
                        let path = Path::new(output_directory);
                        if !path.exists() {
                            Err(Error::OutputDirectoryNotExists)
                        } else if !path.is_dir() {
                            Err(Error::WrongOutputDirectory)
                        } else {
                            Ok(output_directory.to_owned())
                        }
                    })
                    .transpose()?,
            )
            .start_time(Local::now())
            .try_init()
    }
}

impl AppConfigBuilder {
    pub fn try_init(&self) -> Result<AppConfig, Error> {
        self.build()?.init_logger()?.init_merge_file()
    }
}

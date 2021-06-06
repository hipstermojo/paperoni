use std::{collections::BTreeSet, fs, num::NonZeroUsize, path::Path};

use chrono::{DateTime, Local};
use clap::{App, AppSettings, Arg, ArgMatches};
use flexi_logger::LevelFilter as LogLevel;

type Error = crate::errors::CliError<AppConfigBuilderError>;

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
                .long("output-directory")
                .short("o")
                .help("Directory to store output epub documents")
                .conflicts_with("output_name")
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
        use crate::logs;
        logs::init_logger(self.log_level, &self.start_time, self.is_logging_to_file)
            .map(|_| self)
            .map_err(Error::LogError)
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
                let direct_urls = arg_matches
                    .values_of("urls")
                    .and_then(|urls| urls.map(url_filter).collect::<Option<BTreeSet<_>>>());
                let file_urls = arg_matches
                    .value_of("file")
                    .map(fs::read_to_string)
                    .transpose()?
                    .and_then(|content| {
                        content
                            .lines()
                            .map(url_filter)
                            .collect::<Option<BTreeSet<_>>>()
                    });
                match (direct_urls, file_urls) {
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
            .is_logging_to_file(arg_matches.is_present("log-to-file"))
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
        self.build()
            .map_err(Error::AppBuildError)?
            .init_logger()?
            .init_merge_file()
    }
}

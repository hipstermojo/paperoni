use std::{fs, num::NonZeroUsize, path::Path};

use chrono::{DateTime, Local};
use clap::{load_yaml, App, ArgMatches};
use flexi_logger::LevelFilter as LogLevel;
use itertools::Itertools;

type Error = crate::errors::CliError<AppConfigBuilderError>;

const DEFAULT_MAX_CONN: usize = 8;

#[derive(derive_builder::Builder, Debug)]
pub struct AppConfig {
    /// Article urls
    pub urls: Vec<String>,
    pub max_conn: usize,
    /// Path to file of multiple articles into a single article
    pub merged: Option<String>,
    // TODO: Change type to Path
    pub output_directory: Option<String>,
    pub log_level: LogLevel,
    pub can_disable_progress_bar: bool,
    pub start_time: DateTime<Local>,
    pub is_logging_to_file: bool,
    pub inline_toc: bool,
    pub css_config: CSSConfig,
    pub export_type: ExportType,
    pub is_inlining_images: bool,
}

impl AppConfig {
    pub fn init_with_cli() -> Result<AppConfig, Error> {
        let yaml_config = load_yaml!("cli_config.yml");
        let app = App::from_yaml(yaml_config).version(clap::crate_version!());
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
                    .and_then(|urls| urls.map(url_filter).collect::<Option<Vec<_>>>())
                    .unwrap_or(Vec::new());
                let file_urls = arg_matches
                    .value_of("file")
                    .map(fs::read_to_string)
                    .transpose()?
                    .and_then(|content| content.lines().map(url_filter).collect::<Option<Vec<_>>>())
                    .unwrap_or(Vec::new());

                let urls = [direct_urls, file_urls]
                    .concat()
                    .into_iter()
                    .unique()
                    .collect_vec();
                if !urls.is_empty() {
                    Ok(urls)
                } else {
                    Err(Error::NoUrls)
                }
            }?)
            .max_conn(match arg_matches.value_of("max-conn") {
                Some(max_conn) => max_conn.parse::<NonZeroUsize>()?.get(),
                None => DEFAULT_MAX_CONN,
            })
            .merged(arg_matches.value_of("output-name").map(|name| {
                let file_ext = format!(".{}", arg_matches.value_of("export").unwrap_or("epub"));
                if name.ends_with(&file_ext) {
                    name.to_owned()
                } else {
                    name.to_string() + &file_ext
                }
            }))
            .can_disable_progress_bar(
                arg_matches.is_present("verbosity") && !arg_matches.is_present("log-to-file"),
            )
            .log_level(match arg_matches.occurrences_of("verbosity") {
                0 => {
                    if !arg_matches.is_present("log-to-file") {
                        LogLevel::Off
                    } else {
                        LogLevel::Debug
                    }
                }
                1 => LogLevel::Error,
                2 => LogLevel::Warn,
                3 => LogLevel::Info,
                4..=u64::MAX => LogLevel::Debug,
            })
            .is_logging_to_file(arg_matches.is_present("log-to-file"))
            .inline_toc(
                (if arg_matches.is_present("inline-toc") {
                    if arg_matches.value_of("export") == Some("epub") {
                        Ok(true)
                    } else {
                        Err(Error::WrongExportInliningToC)
                    }
                } else {
                    Ok(false)
                })?,
            )
            .output_directory(
                arg_matches
                    .value_of("output-directory")
                    .map(|output_directory| {
                        let path = Path::new(output_directory);
                        if !path.exists() {
                            // TODO: Create the directory
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
            .css_config(
                match (
                    arg_matches.is_present("no-css"),
                    arg_matches.is_present("no-header-css"),
                ) {
                    (true, _) => CSSConfig::None,
                    (_, true) => CSSConfig::NoHeaders,
                    _ => CSSConfig::All,
                },
            )
            .export_type({
                let export_type = arg_matches.value_of("export").unwrap_or("epub");
                if export_type == "html" {
                    ExportType::HTML
                } else {
                    ExportType::EPUB
                }
            })
            .is_inlining_images(
                (if arg_matches.is_present("inline-images") {
                    if arg_matches.value_of("export") == Some("html") {
                        Ok(true)
                    } else {
                        Err(Error::WrongExportInliningImages)
                    }
                } else {
                    Ok(false)
                })?,
            )
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

#[derive(Clone, Debug)]
pub enum CSSConfig {
    All,
    NoHeaders,
    None,
}

#[derive(Clone, Debug)]
pub enum ExportType {
    HTML,
    EPUB,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_clap_config_errors() {
        let yaml_config = load_yaml!("cli_config.yml");
        let app = App::from_yaml(yaml_config);

        // It returns Ok when only a url is passed
        let result = app
            .clone()
            .get_matches_from_safe(vec!["paperoni", "http://example.org"]);
        assert!(result.is_ok());

        // It returns an error when no args are passed
        let result = app.clone().get_matches_from_safe(vec!["paperoni"]);
        assert!(result.is_err());
        assert_eq!(
            clap::ErrorKind::MissingArgumentOrSubcommand,
            result.unwrap_err().kind
        );

        // It returns an error when both output-dir and merge are used
        let result = app.clone().get_matches_from_safe(vec![
            "paperoni",
            "http://example.org",
            "--merge",
            "foo",
            "--output-dir",
            "~",
        ]);
        assert!(result.is_err());
        assert_eq!(clap::ErrorKind::ArgumentConflict, result.unwrap_err().kind);

        // It returns an error when both no-css and no-header-css are used
        let result = app.clone().get_matches_from_safe(vec![
            "paperoni",
            "http://example.org",
            "--no-css",
            "--no-header-css",
        ]);
        assert!(result.is_err());
        assert_eq!(clap::ErrorKind::ArgumentConflict, result.unwrap_err().kind);

        // It returns an error when inline-toc is used without merge
        let result = app.clone().get_matches_from_safe(vec![
            "paperoni",
            "http://example.org",
            "--inline-toc",
        ]);
        assert!(result.is_err());
        assert_eq!(
            clap::ErrorKind::MissingRequiredArgument,
            result.unwrap_err().kind
        );

        // It returns an error when inline-images is used without export
        let result = app.clone().get_matches_from_safe(vec![
            "paperoni",
            "http://example.org",
            "--inline-images",
        ]);
        assert!(result.is_err());
        assert_eq!(
            clap::ErrorKind::MissingRequiredArgument,
            result.unwrap_err().kind
        );

        // It returns an error when export is given an invalid value
        let result = app.clone().get_matches_from_safe(vec![
            "paperoni",
            "http://example.org",
            "--export",
            "pdf",
        ]);
        assert!(result.is_err());
        assert_eq!(clap::ErrorKind::InvalidValue, result.unwrap_err().kind);

        // It returns an error when a max-conn is given a negative number.
        let result = app.clone().get_matches_from_safe(vec![
            "paperoni",
            "http://example.org",
            "--max-conn",
            "-1",
        ]);
        assert!(result.is_err());
        // The cli is configured not to accept negative numbers so passing "-1" would have it be read as a flag called 1
        assert_eq!(clap::ErrorKind::UnknownArgument, result.unwrap_err().kind);
    }

    #[test]
    fn test_init_with_cli() {
        let yaml_config = load_yaml!("cli_config.yml");
        let app = App::from_yaml(yaml_config);

        // It returns an error when the urls passed are whitespace
        let matches = app.clone().get_matches_from(vec!["paperoni", ""]);
        let app_config = AppConfig::try_from(matches);
        assert!(app_config.is_err());
        assert_eq!(Error::NoUrls, app_config.unwrap_err());

        // It returns an error when inline-toc is used when exporting to HTML
        let matches = app.clone().get_matches_from(vec![
            "paperoni",
            "http://example.org",
            "--merge",
            "foo",
            "--export",
            "html",
            "--inline-toc",
        ]);
        let app_config = AppConfig::try_from(matches);
        assert!(app_config.is_err());
        assert_eq!(Error::WrongExportInliningToC, app_config.unwrap_err());
        // It returns an Ok when inline-toc is used when exporting to epub
        let matches = app.clone().get_matches_from(vec![
            "paperoni",
            "http://example.org",
            "--merge",
            "foo",
            "--export",
            "epub",
            "--inline-toc",
        ]);
        assert!(AppConfig::try_from(matches).is_ok());

        // It returns an error when inline-images is used when exporting to epub
    }
}

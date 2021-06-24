<p align="center"><img src="./paperoni-dark.png"></p>

<p align="center"><i>Salami not included</i></p>

<div align="center">
    <a href="https://crates.io/crates/paperoni">
        <img alt="crates.io version" src="https://img.shields.io/crates/v/paperoni.svg">
    </a>
</div>

Paperoni is a CLI tool made in Rust for downloading web articles as EPUBs. There is provisional<sup><a href="#pdf-exports">\*</a></sup> support for exporting to PDF as well.

> This project is in an alpha release so it might crash when you use it. Please open an [issue on Github](https://github.com/hipstermojo/paperoni/issues/new) if it does crash.

## Installation

### Precompiled binaries

Check the [releases](https://github.com/hipstermojo/paperoni/releases) page for precompiled binaries. Currently there are only builds for Debian and Arch.

### Installing from crates.io

Paperoni is published on [crates.io](https://crates.io). If you have [cargo](https://github.com/rust-lang/cargo) installed, then run:

```sh
cargo install paperoni --version 0.5.0-alpha1
```

_Paperoni is still in alpha so the `version` flag has to be passed._

### Building from source

This project uses `async/.await` so it should be compiled using a minimum Rust version of 1.33. Preferrably use the latest version of Rust.

```sh
git clone https://github.com/hipstermojo/paperoni.git
cd paperoni
## You can build and install paperoni locally
cargo install --path .
## or use it from within the project
cargo run -- # pass your url here
```

## Usage

```
USAGE:
    paperoni [OPTIONS] [urls]...

OPTIONS:
    -f, --file <file>
            Input file containing links

    -h, --help
            Prints help information

        --inline-toc
            Add an inlined Table of Contents page at the start of the merged article.

        --log-to-file
            Enables logging of events to a file located in .paperoni/logs with a default log level of debug. Use -v to
            specify the logging level
        --max-conn <max_conn>
            The maximum number of concurrent HTTP connections when downloading articles. Default is 8.
            NOTE: It is advised to use as few connections as needed i.e between 1 and 50. Using more connections can end
            up overloading your network card with too many concurrent requests.
    -o, --output-dir <output_directory>
            Directory for saving epub documents

        --merge <output_name>
            Merge multiple articles into a single epub that will be given the name provided

    -V, --version
            Prints version information

    -v
            This takes upto 4 levels of verbosity in the following order.
             - Error (-v)
             - Warn (-vv)
             - Info (-vvv)
             - Debug (-vvvv)
             When this flag is passed, it disables the progress bars and logs to stderr.
             If you would like to send the logs to a file (and enable progress bars), pass the log-to-file flag.

ARGS:
    <urls>...
            Urls of web articles

```

To download a single article pass in its URL

```sh
paperoni https://en.wikipedia.org/wiki/Pepperoni
```

Paperoni also supports passing multiple links as arguments.

```sh
paperoni https://en.wikipedia.org/wiki/Pepperoni https://en.wikipedia.org/wiki/Salami
```

Alternatively, if you are on a Unix-like OS, you can simply do something like this:

```sh
cat links.txt | xargs paperoni
```

These can also be read from a file using the `-f/--file` flag.

```sh
paperoni -f links.txt
```

### Merging articles

By default, Paperoni generates an epub file for each link. You can also merge multiple links
into a single epub using the `merge` flag and specifying the output file.

```sh
paperoni -f links.txt --merge out.epub
```

### Logging events

Logging is disabled by default. This can be activated by either using the `-v` flag or `--log-to-file` flag. If the `--log-to-file` flag is passed the logs are sent to a file in the default Paperoni directory `.paperoni/logs` which is on your home directory. The `-v` flag configures the verbosity levels such that:

```
-v Logs only the error level
-vv Logs only the warn level
-vvv Logs only the info level
-vvvv Logs only the debug level
```

If only the `-v` flag is passed, the progress bars are disabled. If both `-v` and `--log-to-file` are passed then the progress bars will still be shown.

## How it works

The URL passed to Paperoni is fetched and the returned HTML response is passed to the extractor.
This extractor retrieves a possible article using a [custom port](https://github.com/hipstermojo/paperoni/blob/master/src/moz_readability/mod.rs) of the [Mozilla Readability algorithm](https://github.com/mozilla/readability). This article is then saved in an EPUB.

> The port of the algorithm is still unstable as well so it is not fully compatible with all the websites that can be extracted using Readability.

## How it (currently) doesn't work

This program is still in alpha so a number of things won't work:

- Websites that only run with JavaScript cannot be extracted.
- Website articles that cannot be extracted by Readability cannot be extracted by Paperoni either.
- Code snippets on Medium articles that are lazy loaded will not appear in the EPUB.

There are also web pages it won't work on in general such as Twitter and Reddit threads.

## PDF exports

As of version 0.5-alpha1, you can now export to PDF using a third party tool. This requires that you install [Calibre](https://calibre-ebook.com/) which comes with a ebook conversion. You can convert the epub to a pdf through the terminal with `ebook-convert`:

```sh
# Assuming the downloaded epub was called foo.epub
ebook-convert foo.epub foo.pdf
```

Alternatively, you can use the Calibre GUI to do the file conversion.

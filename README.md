<p align="center"><img src="./paperoni-dark.png"></p>

<p align="center"><i>Salami not included</i></p>

<div align="center">
    <a href="https://crates.io/crates/paperoni">
        <img alt="crates.io version" src="https://img.shields.io/crates/v/paperoni.svg">
    </a>
</div>

Paperoni is a CLI tool made in Rust for downloading web articles as EPUB or HTML files. There is provisional<sup><a href="#pdf-exports">\*</a></sup> support for exporting to PDF as well.

> This project is in an alpha release so it might crash when you use it. Please open an [issue on Github](https://github.com/hipstermojo/paperoni/issues/new) if it does crash.

## Installation

### Precompiled binaries

Check the [releases](https://github.com/hipstermojo/paperoni/releases) page for precompiled binaries. Currently there are only builds for Debian and Arch.

### Installing from crates.io

Paperoni is published on [crates.io](https://crates.io). If you have [cargo](https://github.com/rust-lang/cargo) installed, then run:

```sh
cargo install paperoni --version 0.6.1-alpha1
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
        --export <type>
            Specify the file type of the export. The type must be in lower case. [default: epub]  [possible values:
            html, epub]
    -f, --file <file>
            Input file containing links

    -h, --help
            Prints help information

        --inline-images
            Inlines the article images when exporting to HTML using base64.
            This is used when you do not want a separate folder created for images during HTML export.
            NOTE: It uses base64 encoding on the images which results in larger HTML export sizes as each image
            increases in size by about 25%-33%.
        --inline-toc
            Add an inlined Table of Contents page at the start of the merged article. This does not affect the Table of Contents navigation
        --log-to-file
            Enables logging of events to a file located in .paperoni/logs with a default log level of debug. Use -v to
            specify the logging level
        --max-conn <max-conn>
            The maximum number of concurrent HTTP connections when downloading articles. Default is 8.
            NOTE: It is advised to use as few connections as needed i.e between 1 and 50. Using more connections can end
            up overloading your network card with too many concurrent requests.
        --no-css
            Removes the stylesheets used in the EPUB generation.
            The EPUB file will then be laid out based on your e-reader's default stylesheets.
            Images and code blocks may overflow when this flag is set and layout of generated
            PDFs will be affected. Use --no-header-css if you want to only disable the styling on headers.
        --no-header-css
            Removes the header CSS styling but preserves styling of images and codeblocks. To remove all the default
            CSS, use --no-css instead.
        --merge <output-name>
            Merge multiple articles into a single epub that will be given the name provided

    -o, --output-dir <output_directory>
            Directory to store output epub documents

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

### Exporting articles

By default, Paperoni exports to EPUB files but you can change to HTML by passing the `--export html` flag.

```sh
paperoni https://en.wikipedia.org/wiki/Pepperoni --export html
```

HTML exports allow you to read the articles as plain HTML documents on your browser but can also be used to convert to PDF as explained [here](#).

When exporting to HTML, Paperoni will download the article's images to a folder named similar to the article. Therefore the folder structure would look like this for the command ran above:

```
.
├── Pepperoni - Wikipedia
│   ├── 1a9f886e9b58db72e0003a2cd52681d8.png
│   ├── 216f8a4265a1ceb3f8cfba4c2f9057b1.jpeg
│   ...
└── Pepperoni - Wikipedia.html
```

If you would instead prefer to have the images inlined directly to the HTML export, pass the `inline-images` flag, i.e.:

```sh
paperoni https://en.wikipedia.org/wiki/Pepperoni --export html --inline-images
```

This is especially useful when exporting multiple links.

**NOTE**: The inlining of images for HTML exports uses base64 encoding which is known to increase the overall size of images by about 25% to 33%.

### Disabling CSS

The `no-css` and `no-header-css` flags can be used to remove the default styling added by Paperoni. Refer to `--help` to see the usage of the flags.

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

PDF conversion can be done using a third party tool. There are 2 options to do so:

### EPUB to PDF

This requires that you install [Calibre](https://calibre-ebook.com/) which comes with a ebook conversion. You can convert the epub to a pdf through the terminal with `ebook-convert`:

```sh
# Assuming the downloaded epub was called foo.epub
ebook-convert foo.epub foo.pdf
```

Alternatively, you can use the Calibre GUI to do the file conversion.

### HTML to PDF

The recommended approach is to use [Weasyprint](https://weasyprint.org/start/), a free and open-source tool that converts HTML documents to PDF. It is available on Linux, MacOS and Windows. Using the CLI, it can be done as follows:

```sh
paperoni https://en.wikipedia.org/wiki/Pepperoni --export html
weasyprint "Pepperoni - Wikipedia.html" Pepperoni.pdf
```

Inlining images is not mandatory as Weasyprint will be able to find the files on its own.

### Comparison of PDF conversion methods

Either of the conversion methods is sufficient for most use cases. The main differences are listed below:
| | EPUB to PDF | HTML to PDF |
|----------------------|----------------------------|------------------|
| Wrapping code blocks | Yes | No |
| CSS customization | No | Yes |
| Generated file size | Slightly larger | Slightly smaller |

The difference in file size is due to the additional fonts added to the PDF file by `ebook-convert`.

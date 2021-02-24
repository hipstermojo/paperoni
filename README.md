<p align="center"><img src="./paperoni-dark.png"></p>

<p align="center"><i>Salami not included</i></p>

Paperoni is a web article downloader written in Rust. The downloaded articles are then exported as EPUB files.

> This project is in an alpha release so it might crash when you use it. Please open an [issue on Github](https://github.com/hipstermojo/paperoni/issues/new) if it does crash.

## Installation

### Precompiled binaries

Check the [releases](https://github.com/hipstermojo/paperoni/releases) page for precompiled binaries. Currently there are only builds for Debian and Arch.

### Installing from crates.io

Paperoni is published on [crates.io](https://crates.io). If you have [cargo](https://github.com/rust-lang/cargo) installed, then run:

```sh
cargo install paperoni --version 0.3.0-alpha1
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

## How it works

The URL passed to Paperoni is fetched and the returned HTML response is passed to the extractor.
This extractor retrieves a possible article using a port of the [Mozilla Readability algorithm](https://github.com/mozilla/readability). This article is then saved in an EPUB.

> The port of the algorithm is still unstable as well so it is not fully compatible with all the websites that can be extracted using Readability.

## How it (currently) doesn't work

This program is still in alpha so a number of things won't work:

- Websites that only run with JavaScript cannot be extracted.
- Website articles that cannot be extracted by Readability cannot be extracted by Paperoni either.
- Code snippets on Medium articles that are lazy loaded will not appear in the EPUB.

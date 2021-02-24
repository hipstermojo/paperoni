<p align="center"><img src="./paperoni-dark.png"></p>

<p align="center"><i>Salami not included</i></p>

Paperoni is a web article downloader written in Rust. The downloaded articles are then exported as EPUB files.

> This project is in an alpha release so it is pretty unstable.

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

These can also be read from a file using the `-f` flag.

```sh
paperoni -f links.txt
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

## Running locally

### Precompiled binaries

Check the [releases](https://github.com/hipstermojo/paperoni/releases) page for precompiled binaries. Currently there are only builds for Debian and Arch.

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

#[macro_use]
extern crate lazy_static;

use std::{fs::File, io::Read};

use async_std::task;
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};
use url::Url;

mod cli;
mod extractor;
mod moz_readability;

use extractor::Extractor;
fn main() {
    let app = cli::cli_init();
    let arg_matches = app.get_matches();
    let mut urls: Vec<String> = match arg_matches.value_of("file") {
        Some(file_name) => {
            if let Ok(mut file) = File::open(file_name) {
                let mut content = String::new();
                match file.read_to_string(&mut content) {
                    Ok(_) => content.lines().map(|line| line.to_owned()).collect(),
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
        urls.extend(vals.map(|val| val.to_string()));
    }

    if !urls.is_empty() {
        download(urls);
    }
}

type HTMLResource = (String, String);

async fn fetch_url(url: &str) -> Result<HTMLResource, Box<dyn std::error::Error + Send + Sync>> {
    let client = surf::Client::new();
    println!("Fetching...");

    let mut redirect_count: u8 = 0;
    let base_url = Url::parse(&url)?;
    let mut url = base_url.clone();
    while redirect_count < 5 {
        redirect_count += 1;
        let req = surf::get(&url);
        let mut res = client.send(req).await?;
        if res.status().is_redirection() {
            if let Some(location) = res.header(surf::http::headers::LOCATION) {
                match Url::parse(location.last().as_str()) {
                    Ok(valid_url) => url = valid_url,
                    Err(e) => match e {
                        url::ParseError::RelativeUrlWithoutBase => {
                            url = base_url.join(location.last().as_str())?
                        }
                        e => return Err(e.into()),
                    },
                };
            }
        } else if res.status().is_success() {
            if let Some(mime) = res.content_type() {
                if mime.essence() == "text/html" {
                    return Ok((url.to_string(), res.body_string().await?));
                } else {
                    return Err(format!(
                        "Invalid HTTP response. Received {} instead of text/html",
                        mime.essence()
                    )
                    .into());
                }
            } else {
                return Err("Unknown HTTP response".into());
            }
        } else {
            return Err(format!("Request failed: HTTP {}", res.status()).into());
        }
    }
    Err("Unable to fetch HTML".into())
}

fn download(urls: Vec<String>) {
    let mut async_url_tasks = Vec::with_capacity(urls.len());
    for url in urls {
        async_url_tasks.push(task::spawn(async move { fetch_url(&url).await }));
    }
    task::block_on(async {
        for url_task in async_url_tasks {
            match url_task.await {
                Ok((url, html)) => {
                    println!("Extracting");
                    let mut extractor = Extractor::from_html(&html);
                    extractor.extract_content(&url);
                    if extractor.article().is_some() {
                        extractor
                            .download_images(&Url::parse(&url).unwrap())
                            .await
                            .expect("Unable to download images");
                        let file_name = format!(
                            "{}.epub",
                            extractor
                                .metadata()
                                .title()
                                .replace("/", " ")
                                .replace("\\", " ")
                        );
                        let mut out_file = File::create(&file_name).unwrap();
                        let mut html_buf = Vec::new();
                        extractor::serialize_to_xhtml(extractor.article().unwrap(), &mut html_buf)
                            .expect("Unable to serialize to xhtml");
                        let html_buf = std::str::from_utf8(&html_buf).unwrap();
                        let mut epub = EpubBuilder::new(ZipLibrary::new().unwrap()).unwrap();
                        if let Some(author) = extractor.metadata().byline() {
                            epub.metadata("author", author.replace("&", "&amp;"))
                                .unwrap();
                        }
                        epub.metadata("title", extractor.metadata().title().replace("&", "&amp;"))
                            .unwrap();
                        epub.add_content(EpubContent::new("index.xhtml", html_buf.as_bytes()))
                            .unwrap();
                        for img in extractor.img_urls {
                            let mut file_path = std::env::temp_dir();
                            file_path.push(&img.0);

                            let img_buf = File::open(&file_path).expect("Can't read file");
                            epub.add_resource(
                                file_path.file_name().unwrap(),
                                img_buf,
                                img.1.unwrap(),
                            )
                            .unwrap();
                        }
                        epub.generate(&mut out_file).unwrap();
                        println!("Created {:?}", file_name);
                    }
                }
                Err(e) => println!("{}", e),
            }
        }
    })
}

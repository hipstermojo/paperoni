use async_std::io::prelude::*;
use async_std::task;
use async_std::{fs::File, stream};
use futures::StreamExt;
use indicatif::ProgressBar;
use log::warn;
use log::{debug, info};
use url::Url;

use crate::cli::AppConfig;
use crate::errors::{ErrorKind, ImgError, PaperoniError};
use crate::extractor::Extractor;
type HTMLResource = (String, String);

pub fn download(
    app_config: &AppConfig,
    bar: &ProgressBar,
    partial_downloads: &mut Vec<PartialDownload>,
    errors: &mut Vec<PaperoniError>,
) -> Vec<Extractor> {
    task::block_on(async {
        let urls_iter = app_config.urls.iter().map(|url| fetch_html(url));
        let mut responses = stream::from_iter(urls_iter).buffered(app_config.max_conn);
        let mut articles = Vec::new();
        while let Some(fetch_result) = responses.next().await {
            match fetch_result {
                Ok((url, html)) => {
                    debug!("Extracting {}", &url);
                    let mut extractor = Extractor::from_html(&html, &url);
                    bar.set_message("Extracting...");
                    match extractor.extract_content() {
                        Ok(_) => {
                            extractor.extract_img_urls();
                            if let Err(img_errors) =
                                download_images(&mut extractor, &Url::parse(&url).unwrap(), &bar)
                                    .await
                            {
                                partial_downloads
                                    .push(PartialDownload::new(&url, extractor.metadata().title()));
                                warn!(
                                    "{} image{} failed to download for {}",
                                    img_errors.len(),
                                    if img_errors.len() > 1 { "s" } else { "" },
                                    url
                                );
                                for img_error in img_errors {
                                    warn!(
                                        "{}\n\t\tReason {}",
                                        img_error.url().as_ref().unwrap(),
                                        img_error
                                    );
                                }
                            }
                            articles.push(extractor);
                        }
                        Err(mut e) => {
                            e.set_article_source(&url);
                            errors.push(e);
                        }
                    }
                }
                Err(e) => errors.push(e),
            }
            bar.inc(1);
        }
        articles
    })
}

pub async fn fetch_html(url: &str) -> Result<HTMLResource, PaperoniError> {
    let client = surf::Client::new();
    debug!("Fetching {}", url);

    let process_request = async {
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
                        Ok(valid_url) => {
                            info!("Redirecting {} to {}", url, valid_url);
                            url = valid_url
                        }
                        Err(e) => match e {
                            url::ParseError::RelativeUrlWithoutBase => {
                                match base_url.join(location.last().as_str()) {
                                    Ok(joined_url) => {
                                        info!("Redirecting {} to {}", url, joined_url);
                                        url = joined_url;
                                    }
                                    Err(e) => return Err(e.into()),
                                }
                            }
                            e => return Err(e.into()),
                        },
                    };
                }
            } else if res.status().is_success() {
                if let Some(mime) = res.content_type() {
                    if mime.essence() == "text/html" {
                        debug!("Successfully fetched {}", url);
                        return Ok((url.to_string(), res.body_string().await?));
                    } else {
                        let msg = format!(
                            "Invalid HTTP response. Received {} instead of text/html",
                            mime.essence()
                        );

                        return Err(ErrorKind::HTTPError(msg).into());
                    }
                } else {
                    return Err(ErrorKind::HTTPError("Unknown HTTP response".to_owned()).into());
                }
            } else {
                let msg = format!("Request failed: HTTP {}", res.status());
                return Err(ErrorKind::HTTPError(msg).into());
            }
        }
        Err(ErrorKind::HTTPError("Unable to fetch HTML".to_owned()).into())
    };

    process_request.await.map_err(|mut error: PaperoniError| {
        error.set_article_source(url);
        error
    })
}

type ImgItem<'a> = (&'a str, String, Option<String>);

async fn process_img_response<'a>(
    img_response: &mut surf::Response,
    url: &'a str,
) -> Result<ImgItem<'a>, ImgError> {
    if !img_response.status().is_success() {
        let kind = ErrorKind::HTTPError(format!(
            "Non-success HTTP status code ({})",
            img_response.status()
        ));
        return Err(ImgError::with_kind(kind));
    }
    let img_content: Vec<u8> = match img_response.body_bytes().await {
        Ok(bytes) => bytes,
        Err(e) => return Err(e.into()),
    };
    let img_mime = img_response
        .content_type()
        .map(|mime| mime.essence().to_string());
    let img_ext = match img_response
        .content_type()
        .map(|mime| map_mime_subtype_to_ext(mime.subtype()).to_string())
    {
        Some(mime_str) => mime_str,
        None => return Err(ErrorKind::HTTPError("Image has no Content-Type".to_owned()).into()),
    };

    let mut img_path = std::env::temp_dir();
    img_path.push(format!("{}.{}", hash_url(url), &img_ext));
    let mut img_file = match File::create(&img_path).await {
        Ok(file) => file,
        Err(e) => return Err(e.into()),
    };
    match img_file.write_all(&img_content).await {
        Ok(_) => (),
        Err(e) => return Err(e.into()),
    }

    Ok((
        url,
        img_path
            .file_name()
            .map(|os_str_name| {
                os_str_name
                    .to_str()
                    .expect("Unable to get image file name")
                    .to_string()
            })
            .unwrap(),
        img_mime,
    ))
}

pub async fn download_images(
    extractor: &mut Extractor,
    article_origin: &Url,
    bar: &ProgressBar,
) -> Result<(), Vec<ImgError>> {
    if extractor.img_urls.len() > 0 {
        debug!(
            "Downloading {} images for {}",
            extractor.img_urls.len(),
            article_origin
        );
    }
    let img_count = extractor.img_urls.len();

    let imgs_req_iter = extractor
        .img_urls
        .iter()
        .map(|(url, _)| {
            (
                url,
                surf::Client::new()
                    .with(surf::middleware::Redirect::default())
                    .get(get_absolute_url(&url, article_origin)),
            )
        })
        .enumerate()
        .map(|(img_idx, (url, req))| async move {
            bar.set_message(format!(
                "Downloading images [{}/{}]",
                img_idx + 1,
                img_count
            ));
            match req.await {
                Ok(mut img_response) => {
                    let process_response =
                        process_img_response(&mut img_response, url.as_ref()).await;
                    process_response.map_err(|mut e: ImgError| {
                        e.set_url(url);
                        e
                    })
                }
                Err(e) => {
                    let mut img_err: ImgError = e.into();
                    img_err.set_url(url);
                    Err(img_err)
                }
            }
        });

    // A utility closure used when update the value of an image source after downloading is successful
    let replace_existing_img_src = |img_item: ImgItem| -> (String, Option<String>) {
        let (img_url, img_path, img_mime) = img_item;
        let img_ref = extractor
            .article()
            .select_first(&format!("img[src='{}']", img_url))
            .expect("Image node does not exist");
        let mut img_node = img_ref.attributes.borrow_mut();
        *img_node.get_mut("src").unwrap() = img_path.clone();
        // srcset is removed because readers such as Foliate then fail to display
        // the image already downloaded and stored in src
        img_node.remove("srcset");
        (img_path, img_mime)
    };

    let imgs_req_iter = stream::from_iter(imgs_req_iter)
        .buffered(10)
        .collect::<Vec<Result<_, ImgError>>>()
        .await;
    let mut errors = Vec::new();
    let mut replaced_imgs = Vec::new();
    for img_req_result in imgs_req_iter {
        match img_req_result {
            Ok(img_req) => replaced_imgs.push(replace_existing_img_src(img_req)),
            Err(e) => errors.push(e),
        }
    }
    extractor.img_urls = replaced_imgs;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub struct PartialDownload {
    pub link: String,
    pub title: String,
}

impl PartialDownload {
    pub fn new(link: &str, title: &str) -> Self {
        Self {
            link: link.into(),
            title: title.into(),
        }
    }
}

/// Handles getting the extension from a given MIME subtype.
fn map_mime_subtype_to_ext(subtype: &str) -> &str {
    if subtype == ("svg+xml") {
        return "svg";
    } else if subtype == "x-icon" {
        "ico"
    } else {
        subtype
    }
}

/// Utility for hashing URLs. This is used to help store files locally with unique values
fn hash_url(url: &str) -> String {
    format!("{:x}", md5::compute(url.as_bytes()))
}

fn get_absolute_url(url: &str, request_url: &Url) -> String {
    if Url::parse(url).is_ok() {
        url.to_owned()
    } else if url.starts_with("/") {
        Url::parse(&format!(
            "{}://{}",
            request_url.scheme(),
            request_url.host_str().unwrap()
        ))
        .unwrap()
        .join(url)
        .unwrap()
        .into()
    } else {
        request_url.join(url).unwrap().into()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_map_mime_type_to_ext() {
        let mime_subtypes = vec![
            "apng", "bmp", "gif", "x-icon", "jpeg", "png", "svg+xml", "tiff", "webp",
        ];
        let exts = mime_subtypes
            .into_iter()
            .map(|mime_type| map_mime_subtype_to_ext(mime_type))
            .collect::<Vec<_>>();
        assert_eq!(
            vec!["apng", "bmp", "gif", "ico", "jpeg", "png", "svg", "tiff", "webp"],
            exts
        );
    }
}

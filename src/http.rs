use async_std::io::prelude::*;
use async_std::{fs::File, stream};
use futures::StreamExt;
use url::Url;

use crate::extractor::Extractor;

type HTMLResource = (String, String);

pub async fn fetch_url(
    url: &str,
) -> Result<HTMLResource, Box<dyn std::error::Error + Send + Sync>> {
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

pub async fn download_images(
    extractor: &mut Extractor,
    article_origin: &Url,
) -> async_std::io::Result<()> {
    if extractor.img_urls.len() > 0 {
        println!("Downloading images...");
    }

    let imgs_req_iter = extractor
        .img_urls
        .iter()
        .map(|(url, _)| {
            (
                url,
                surf::Client::new().get(get_absolute_url(&url, article_origin)),
            )
        })
        .map(|(url, req)| async move {
            let mut img_response = req.await.expect("Unable to retrieve image");
            let img_content: Vec<u8> = img_response.body_bytes().await.unwrap();
            let img_mime = img_response
                .content_type()
                .map(|mime| mime.essence().to_string());
            let img_ext = img_response
                .content_type()
                .map(|mime| map_mime_subtype_to_ext(mime.subtype()).to_string())
                .unwrap();

            let mut img_path = std::env::temp_dir();
            img_path.push(format!("{}.{}", hash_url(&url), &img_ext));
            let mut img_file = File::create(&img_path)
                .await
                .expect("Unable to create file");
            img_file
                .write_all(&img_content)
                .await
                .expect("Unable to save to file");

            (
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
            )
        });

    // A utility closure used when update the value of an image source after downloading is successful
    let replace_existing_img_src =
        |img_item: (&String, String, Option<String>)| -> (String, Option<String>) {
            let (img_url, img_path, img_mime) = img_item;
            let img_ref = extractor
                .article()
                .as_mut()
                .expect("Unable to get mutable ref")
                .select_first(&format!("img[src='{}']", img_url))
                .expect("Image node does not exist");
            let mut img_node = img_ref.attributes.borrow_mut();
            *img_node.get_mut("src").unwrap() = img_path.clone();
            // srcset is removed because readers such as Foliate then fail to display
            // the image already downloaded and stored in src
            img_node.remove("srcset");
            (img_path, img_mime)
        };

    extractor.img_urls = stream::from_iter(imgs_req_iter)
        .buffered(10)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(replace_existing_img_src)
        .collect();
    Ok(())
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
        .into_string()
    } else {
        request_url.join(url).unwrap().into_string()
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

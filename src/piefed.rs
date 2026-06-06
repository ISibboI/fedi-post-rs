use log::{debug, info, warn};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde::Serialize;
use serde_json::Value;
use thiserror::Error;

use crate::{ServerCredentials, ServerPost};

/// The API version to use when posting to PieFed.
pub static API_VERSION: &str = "alpha";

#[derive(Debug, Serialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct CreatePostRequest {
    title: String,
    community_id: i64,
    alt_text: Option<String>,
    body: Option<String>,
    url: Option<String>,
    nsfw: Option<bool>,
    ai_generated: Option<bool>,
    language_id: Option<i64>,
}

/// Errors that can occur when posting to PieFed.
#[derive(Debug, Error)]
pub enum PieFedError {
    /// An error occurred during HTTP communication with the PieFed server.
    #[error("HTTP communication failed: {0}")]
    ReqwestError(#[from] reqwest::Error),

    /// Error while logging into PieFed.
    #[error("Login failed. Status code {response_status} and response body {response_body:#?}")]
    Login {
        /// HTTP status code returned by the PieFed server.
        response_status: StatusCode,
        /// The HTTP response body returned by the PieFed server, parsed as JSON.
        response_body: Value,
    },

    /// Error while finding a community in PieFed.
    #[error(
        "Community {community_name} not found. Status code {response_status} and response body {response_body:#?}"
    )]
    CommunityNotFound {
        /// The name of the community that was searched for.
        community_name: String,
        /// HTTP status code returned by the PieFed server.
        response_status: StatusCode,
        /// The HTTP response body returned by the PieFed server, parsed as JSON.
        response_body: Value,
    },

    /// Error while finding a language in PieFed.
    #[error("Language {language_code} not found. Availabe languages are {available_languages:?}")]
    LanguageNotFound {
        /// The language code that was searched for.
        language_code: String,
        /// The available language codes on the server.
        available_languages: Vec<String>,
    },

    /// Error while creating a post in PieFed.
    #[error(
        "Failed to create post. Status code {response_status} and response body {response_body:#?}"
    )]
    Post {
        /// HTTP status code returned by the PieFed server.
        response_status: StatusCode,
        /// The HTTP response body returned by the PieFed server, parsed as JSON.
        response_body: Value,
    },

    /// The community id returned by the PieFed server is not an integer.
    #[error("Community id {community_id:#?} is not an integer.")]
    CommunityIdNotInteger {
        /// The offending community id value.
        community_id: Value,
    },

    /// The all languages field returned by the PieFed server is not an array.
    #[error("All languages field is not an array. Value: {all_languages:#?}")]
    AllLanguagesNotArray {
        /// The offending all languages value.
        all_languages: Value,
    },

    /// A language code returned by the PieFed server is not a string.
    #[error("Language code {language_code:#?} is not a string.")]
    LanguageCodeNotString {
        /// The offending language code value.
        language_code: Value,
    },

    /// Fetching the site meta information from the PieFed server failed.
    #[error(
        "Unable to fetch site meta information. Status code {response_status} and response body {response_body:#?}"
    )]
    SiteRequestFailed {
        /// HTTP status code returned by the PieFed server.
        response_status: StatusCode,
        /// The HTTP response body returned by the PieFed server, parsed as JSON.
        response_body: Value,
    },

    /// The language id returned by the PieFed server is not an integer.
    #[error("Language id {language_id:#?} is not an integer.")]
    LanguageIdNotInteger {
        /// The offending language id value.
        language_id: Value,
    },
}

pub(super) async fn post_to_piefed(
    credentials: &ServerCredentials,
    post: ServerPost,
) -> Result<(), PieFedError> {
    let client = Client::new();
    let mut post_headers = HeaderMap::new();
    post_headers.insert("Content-Type", "application/json".parse().unwrap());
    post_headers.insert("Accept", "application/json".parse().unwrap());
    let mut get_headers = HeaderMap::new();
    get_headers.insert("Accept", "application/json".parse().unwrap());

    let jwt;
    {
        info!("Logging into piefed at {}", credentials.domain);
        let request_body = LoginRequest {
            username: credentials.username.clone(),
            password: credentials.password.clone(),
        };

        let response = client
            .post(format!(
                "https://{}/api/{API_VERSION}/user/login",
                credentials.domain
            ))
            .headers(post_headers.clone())
            .json(&request_body)
            .send()
            .await?;

        debug!("Login request body: {:?}", request_body);
        debug!("Login response: {:?}", response);
        let status = response.status();
        let body = response.json::<serde_json::Value>().await?;
        debug!("Login response body: {:?}", body);

        if status != 200 {
            return Err(PieFedError::Login {
                response_status: status,
                response_body: body,
            });
        }

        jwt = body["jwt"].to_string();
        info!("JWT: {}", jwt);
    }

    post_headers.insert("Authorization", format!("Bearer {}", jwt).parse().unwrap());
    get_headers.insert("Authorization", format!("Bearer {}", jwt).parse().unwrap());

    let community_id;
    {
        info!("Getting community id for {}", post.community);
        let response = client
            .get(format!(
                "https://{}/api/{API_VERSION}/community?name={}",
                credentials.domain, post.community,
            ))
            .headers(get_headers.clone())
            .send()
            .await?;

        debug!("GetCommunity response: {:?}", response);
        let status = response.status();
        let body = response.json::<serde_json::Value>().await?;
        debug!("GetCommunity response body: {:?}", body);

        if status != 200 {
            return Err(PieFedError::CommunityNotFound {
                community_name: post.community.clone(),
                response_status: status,
                response_body: body,
            });
        }

        community_id = body["community_view"]["community"]["id"]
            .as_i64()
            .ok_or_else(|| PieFedError::CommunityIdNotInteger {
                community_id: body["community_view"]["community"]["id"].clone(),
            })?;
        info!("Community id for {} is {}", post.community, community_id,);
    }

    let language_id;
    if let Some(language_code) = &post.language {
        info!("Getting language id for {}", language_code);
        let response = client
            .get(format!(
                "https://{}/api/{API_VERSION}/site",
                credentials.domain
            ))
            .headers(get_headers.clone())
            .send()
            .await?;

        debug!("GetSite response: {:?}", response);
        let status = response.status();
        let body = response.json::<serde_json::Value>().await?;
        debug!("GetSite response body: {:?}", body);

        if status != 200 {
            return Err(PieFedError::SiteRequestFailed {
                response_status: status,
                response_body: body,
            });
        }

        let all_languages = body["site"]["all_languages"].as_array().ok_or_else(|| {
            PieFedError::AllLanguagesNotArray {
                all_languages: body["site"]["all_languages"].clone(),
            }
        })?;
        let language_id_value = &all_languages
            .iter()
            .find(|language| language["code"].as_str() == Some(language_code.as_str()))
            .ok_or_else(|| PieFedError::LanguageNotFound {
                language_code: language_code.clone(),
                available_languages: all_languages
                    .iter()
                    .map(|language| language["code"].to_string())
                    .collect(),
            })?["id"];

        language_id =
            Some(
                language_id_value
                    .as_i64()
                    .ok_or_else(|| PieFedError::LanguageIdNotInteger {
                        language_id: language_id_value.clone(),
                    })?,
            );
        info!(
            "Language id for {} is {}",
            language_code,
            language_id.unwrap(),
        );
    } else {
        info!("No language specified, skipping getting language id");
        language_id = None;
    }

    {
        info!("Posting to PieFed");
        debug!("Post data: {:?}", post);

        let ServerPost {
            community: _community,
            title,
            url,
            body,
            language: _language,
            alt_text,
            nsfw,
            nsfl,
            ai_generated,
            custom_thumbnail,
        } = post;

        let request_body = CreatePostRequest {
            title,
            community_id,
            alt_text,
            body,
            url,
            nsfw,
            ai_generated,
            language_id,
        };

        if let Some(nsfl) = nsfl {
            warn!(
                "The nsfl field is set to {nsfl}, but PieFed does not support NSFL. Ignoring this field."
            );
        }
        if let Some(custom_thumbnail) = custom_thumbnail {
            warn!(
                "The custom_thumbnail field is set to {custom_thumbnail}, but PieFed does not support custom thumbnails. Ignoring this field."
            );
        }

        let response = client
            .post(format!(
                "https://{}/api/{API_VERSION}/post",
                credentials.domain
            ))
            .headers(post_headers.clone())
            .json(&request_body)
            .send()
            .await?;

        debug!("CreatePost request body: {:?}", request_body);
        debug!("CreatePost response: {:?}", response);
        let status = response.status();
        let body = response.json::<serde_json::Value>().await.unwrap();
        info!("CreatePost response body: {:?}", body);

        if status != 200 {
            return Err(PieFedError::Post {
                response_status: status,
                response_body: body,
            });
        }

        info!("Post created successfully");
        Ok(())
    }
}

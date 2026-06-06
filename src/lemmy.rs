use lemmy_client::{
    ClientOptions, LemmyClient, LemmyRequest,
    lemmy_api_common::{community::GetCommunity, person::Login, post::CreatePost},
};
use log::{info, warn};
use thiserror::Error;

use crate::{ServerCredentials, ServerPost};

/// Errors that can occur when posting to Lemmy.
#[derive(Debug, Error)]
pub enum LemmyError {
    /// Error while logging into Lemmy.
    #[error("Login error: {0}")]
    Login(lemmy_client::lemmy_api_common::lemmy_utils::error::LemmyErrorType),

    /// Error while finding a community in Lemmy.
    #[error("Find community error: {0}")]
    Community(lemmy_client::lemmy_api_common::lemmy_utils::error::LemmyErrorType),

    /// Error while finding a language in Lemmy.
    #[error("Find language error: {0}")]
    Language(lemmy_client::lemmy_api_common::lemmy_utils::error::LemmyErrorType),

    /// Error while creating a post in Lemmy.
    #[error("Post error: {0}")]
    Post(lemmy_client::lemmy_api_common::lemmy_utils::error::LemmyErrorType),

    /// Error indicating that login failed because no JWT was returned.
    #[error("Login failed. No JWT returned.")]
    LoginReturnedNoJwt,

    /// Error indicating that the specified language was not found.
    #[error(
        "Language {language_code} not found. Availabe language codes are {available_languages:?}"
    )]
    LanguageNotFound {
        /// The language code as specified in the post.
        language_code: String,
        /// The available language codes on the server.
        available_languages: Vec<String>,
    },
}

pub(super) async fn post_to_lemmy(
    credentials: &ServerCredentials,
    post: ServerPost,
) -> Result<(), LemmyError> {
    info!("Logging into lemmy at {}", credentials.domain);
    let client = LemmyClient::new(ClientOptions {
        domain: credentials.domain.clone(),
        secure: true,
    });
    let jwt = client
        .login(Login {
            username_or_email: credentials.username.clone().into(),
            password: credentials.password.clone().into(),
            totp_2fa_token: None,
        })
        .await
        .map_err(LemmyError::Login)?
        .jwt
        .ok_or(LemmyError::LoginReturnedNoJwt)?;
    info!("JWT: {}", &*jwt);

    info!("Getting community id for {}", post.community);
    let community_id = client
        .get_community(GetCommunity {
            id: None,
            name: Some(post.community.clone()),
        })
        .await
        .map_err(LemmyError::Community)?
        .community_view
        .community
        .id;
    info!("Community id for {} is {}", post.community, community_id.0);

    let language_id;
    if let Some(language_code) = post.language.as_ref() {
        info!("Getting language id for {}", language_code);
        let all_languages = &client
            .get_site(())
            .await
            .map_err(LemmyError::Language)?
            .all_languages;
        language_id = Some(
            all_languages
                .iter()
                .find(|lang| &lang.code == language_code)
                .ok_or_else(|| LemmyError::LanguageNotFound {
                    language_code: language_code.clone(),
                    available_languages: all_languages
                        .iter()
                        .map(|language| language.code.clone())
                        .collect(),
                })?
                .id,
        );
        info!(
            "Language id for {} is {}",
            language_code,
            language_id.unwrap().0,
        );
    } else {
        info!("No language specified, skipping getting language id");
        language_id = None;
    }

    info!("Posting to lemmy");
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
    let create_post = CreatePost {
        name: title,
        community_id,
        url,
        body,
        alt_text,
        honeypot: None,
        nsfw,
        language_id,
        custom_thumbnail,
    };

    if let Some(nsfl) = nsfl {
        warn!(
            "The nsfl field is set to {nsfl}, but Lemmy does not support the nsfl field. Ignoring this field."
        );
    }
    if let Some(ai_generated) = ai_generated {
        warn!(
            "The ai_generated field is set to {ai_generated}, but Lemmy does not support the ai_generated field. Ignoring this field."
        );
    }

    info!("Posting {create_post:?}");
    client
        .create_post(LemmyRequest {
            body: create_post,
            jwt: Some(jwt.into_inner()),
        })
        .await
        .map_err(LemmyError::Post)?;

    info!("Post created successfully");
    Ok(())
}

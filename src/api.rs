use serde::de;
use std::fmt::Display;
use std::sync::Arc;
use ureq::SendBody;
use ureq::http::{StatusCode, header};
use url::Url;

// TODO: allow overrides in settings?
pub const BAAM_HOST: &str = "192.168.12.78:44321";
// TODO: re-enable
const ENABLE_CERT_VERIFICATION: bool = false;

pub struct ApiContext {
    pub base_url: Url,
}

pub struct Api {
    context: Arc<ApiContext>,
    agent: ureq::Agent,
}

impl Api {
    pub fn new() -> Self {
        let context = ApiContext {
            base_url: Url::parse(&format!("https://{}/", BAAM_HOST)).unwrap(),
        };

        let config = ureq::config::Config::builder()
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::Rustls)
                    .unversioned_rustls_crypto_provider(Arc::new(rustls_rustcrypto::provider()))
                    .disable_verification(!ENABLE_CERT_VERIFICATION)
                    .build(),
            )
            .user_agent("baam-3ds-rs/1.0")
            .build();
        let agent = config.new_agent();

        Self {
            context: Arc::new(context),
            agent,
        }
    }

    pub async fn make_request<R: ApiRequest>(
        &self,
        request: R,
    ) -> Result<R::Response, ureq::Error> {
        let context = self.context.clone();
        let agent = self.agent.clone();

        // let's hope that the default stack size will be enough for us (doubt)
        blocking::unblock(move || -> Result<R::Response, ureq::Error> {
            let request = request
                .make_request(&context)
                .expect("Building a request failed");

            let mut response = agent.run(request)?;
            if response.status() != StatusCode::OK {
                return Err(ureq::Error::StatusCode(response.status().as_u16()));
            }

            response.body_mut().read_json()
        })
        .await
    }

    pub async fn redeem_login_token(
        &self,
        login_token: String,
    ) -> Result<RedeemLoginTokenResponse, ureq::Error> {
        self.make_request(RedeemLoginToken { login_token }).await
    }

    pub async fn get_me(&self, acces_token: String) -> Result<MeResponse, ureq::Error> {
        self.make_request(Me {
            access_token: acces_token,
        })
        .await
    }

    pub async fn submit_challenge(
        &self,
        access_token: String,
        body: SubmitChallengeBody,
    ) -> Result<SubmitChallengeResponse, ureq::Error> {
        self.make_request(SubmitChallenge { access_token, body })
            .await
    }
}

pub trait ApiRequest: Send + 'static {
    type Response: for<'de> serde::Deserialize<'de> + Send + 'static;

    fn make_request(
        self,
        context: &ApiContext,
    ) -> ureq::http::Result<ureq::http::Request<SendBody<'static>>>;
}

pub struct RedeemLoginToken {
    pub login_token: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(transparent)]
pub struct RedeemLoginTokenResponse {
    pub access_token: String,
}

impl ApiRequest for RedeemLoginToken {
    type Response = RedeemLoginTokenResponse;

    fn make_request(
        self,
        context: &ApiContext,
    ) -> ureq::http::Result<ureq::http::Request<SendBody<'static>>> {
        ureq::http::Request::post(
            context
                .base_url
                .join("/api/v2/redeem-login-token")
                .unwrap()
                .as_str(),
        )
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", self.login_token),
        )
        .body(SendBody::none())
    }
}

pub struct Me {
    pub access_token: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub name: String,
}

impl ApiRequest for Me {
    type Response = MeResponse;

    fn make_request(
        self,
        context: &ApiContext,
    ) -> ureq::http::Result<ureq::http::Request<SendBody<'static>>> {
        ureq::http::Request::get(context.base_url.join("/api/v2/me").unwrap().as_str())
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", self.access_token),
            )
            .body(SendBody::none())
    }
}

pub struct SubmitChallenge {
    // TODO: the auth should be handled more generically I think
    pub access_token: String,
    pub body: SubmitChallengeBody,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitChallengeBody {
    pub code: String,
    pub secret_code: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitChallengeResponse {
    #[expect(unused)]
    pub session_code: String,
    pub session_title: String,
    pub your_username: String,
    pub attendance_snippet: Vec<String>,
    pub message_of_the_day: Option<ClientMessage>,
}

impl ApiRequest for SubmitChallenge {
    type Response = SubmitChallengeResponse;

    fn make_request(
        self,
        context: &ApiContext,
    ) -> ureq::http::Result<ureq::http::Request<SendBody<'static>>> {
        ureq::http::Request::post(
            context
                .base_url
                .join("/api/v2/submit-challenge")
                .unwrap()
                .as_str(),
        )
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", self.access_token),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(SendBody::from_json(&self.body).unwrap())
    }
}

#[derive(Debug, serde::Deserialize)]
struct ClientMessageVariant {
    pub r#type: String,
    pub value: String,
}

// only captures the plaintext version
// (I am not about to write an HTML parser & renderer)
#[derive(Debug)]
pub struct ClientMessage(Option<String>);

impl<'de> serde::Deserialize<'de> for ClientMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Option<String>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(formatter, "a sequence of client message variants")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut plain = None;

                while let Some(variant) = seq.next_element::<ClientMessageVariant>()? {
                    if variant.r#type == "plain" {
                        plain = Some(variant.value);
                    }
                }

                Ok(plain)
            }
        }

        deserializer.deserialize_seq(Visitor).map(ClientMessage)
    }
}

impl Display for ClientMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(message) = &self.0 {
            writeln!(f, "{}", message)
        } else {
            writeln!(f, "<Message Unsupported>")
        }
    }
}

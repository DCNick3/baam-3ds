use crate::api::{Api, SubmitChallengeBody};
use crate::qr::QrProcessorHandle;
use crate::settings_storage::SETTINGS;
use crate::ui::UiHandle;
use tracing::info;

async fn check_access_token(
    api: &Api,
    ui: &UiHandle,
    access_token: Option<String>,
) -> Option<String> {
    let Some(access_token) = access_token else {
        return None;
    };

    let (token, username) = loop {
        match api.get_me(access_token.clone()).await {
            Ok(me) => {
                info!("Logged in as {}, saving token", me.name);
                break (Some(access_token), Some(me.name));
            }
            Err(ureq::Error::StatusCode(401)) => {
                info!("Access token is invalid, deleting token");
                break (None, None);
            }
            Err(e) => {
                ui.prompt_restart(e).await;
            }
        }
    };

    ui.set_username(username).await;
    SETTINGS.modify(|s| s.access_token = token.clone());

    token
}

async fn retrieve_access_token(api: &Api, ui: &UiHandle, qr: &mut QrProcessorHandle) -> String {
    loop {
        ui.ask_to_scan_login().await;
        let token_candidate = qr.scan_login_token().await;

        let Some(result) = ui
            .handle(api.redeem_login_token(token_candidate).await)
            .await
        else {
            continue;
        };

        let Some(access_token) = check_access_token(api, ui, Some(result.access_token)).await
        else {
            continue;
        };

        return access_token;
    }
}

async fn use_access_token(
    api: &Api,
    ui: &UiHandle,
    qr: &mut QrProcessorHandle,
    access_token: String,
) {
    loop {
        ui.ask_to_scan_challenge().await;
        let challenge = qr.scan_challenge().await;

        match api
            .submit_challenge(
                access_token.clone(),
                SubmitChallengeBody {
                    code: challenge.code,
                    secret_code: challenge.secret_code,
                },
            )
            .await
        {
            Ok(response) => {
                info!("Challenge succeeded: {:?}", response);
                ui.prompt_success(
                    response.session_title,
                    response.attendance_snippet,
                    response.your_username,
                    response.message_of_the_day,
                )
                .await;
            }
            Err(ureq::Error::StatusCode(401)) => {
                info!("It seems that access token has become invalid");
                return;
            }
            Err(e) => ui.notify_error(e).await,
        }
    }
}

async fn flow(ui: UiHandle, mut qr: QrProcessorHandle) {
    let api = Api::new();

    loop {
        let access_token = check_access_token(&api, &ui, SETTINGS.get().access_token).await;
        let access_token = if let Some(access_token) = access_token {
            access_token
        } else {
            info!("No valid access token, prompting the user to retrieve it");
            retrieve_access_token(&api, &ui, &mut qr).await
        };

        info!("Got a valid access token, using it to submit the challenge");
        use_access_token(&api, &ui, &mut qr, access_token).await;
    }
}

pub fn exec_flow(ui: UiHandle, qr: QrProcessorHandle) {
    futures::executor::block_on(flow(ui, qr))
}

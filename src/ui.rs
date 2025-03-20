use crate::api::ClientMessage;
use std::time::Duration;

pub enum UiCommand {
    NotifyError(String),
    PromptRestart(String, oneshot::Sender<()>),

    AskToScanLogin,
    AskToScanChallenge,
    PromptSuccess(
        String,
        Vec<String>,
        String,
        Option<ClientMessage>,
        oneshot::Sender<()>,
    ),

    SetUsername(Option<String>),
    SetNetState(bool),

    FinishedProcessing(Duration),
}

#[derive(Clone)]
pub struct UiHandle {
    sender: async_channel::Sender<UiCommand>,
}

impl UiHandle {
    pub fn new(sender: async_channel::Sender<UiCommand>) -> Self {
        Self { sender }
    }

    pub async fn notify_error<E>(&self, error: E)
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let error = anyhow::Error::from(error);
        self.sender
            .send(UiCommand::NotifyError(error.to_string()))
            .await
            .unwrap();
    }

    pub async fn handle<T, E>(&self, result: Result<T, E>) -> Option<T>
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                self.notify_error(e).await;
                None
            }
        }
    }

    pub async fn prompt_restart<E>(&self, error: E)
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let error = anyhow::Error::from(error);

        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(UiCommand::PromptRestart(error.to_string(), sender))
            .await
            .unwrap();

        receiver.await.unwrap();
    }

    pub async fn ask_to_scan_login(&self) {
        self.sender.send(UiCommand::AskToScanLogin).await.unwrap();
    }

    pub async fn ask_to_scan_challenge(&self) {
        self.sender
            .send(UiCommand::AskToScanChallenge)
            .await
            .unwrap();
    }

    pub async fn prompt_success(
        &self,
        session_name: String,
        attendance_snippet: Vec<String>,
        your_username: String,
        motd: Option<ClientMessage>,
    ) {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(UiCommand::PromptSuccess(
                session_name,
                attendance_snippet,
                your_username,
                motd,
                sender,
            ))
            .await
            .unwrap();

        receiver.await.unwrap();
    }

    pub async fn set_username(&self, username: Option<String>) {
        self.sender
            .send(UiCommand::SetUsername(username))
            .await
            .unwrap();
    }

    pub async fn set_net_state(&self, busy: bool) {
        self.sender
            .send(UiCommand::SetNetState(busy))
            .await
            .unwrap();
    }

    pub async fn finished_processing(&self, duration: Duration) {
        self.sender
            .send(UiCommand::FinishedProcessing(duration))
            .await
            .unwrap();
    }
}

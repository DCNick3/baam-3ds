use crate::api::ClientMessage;
use render::BreadcrumbRequest;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32};

pub enum UiCommand {
    NotifyError(String),
    PromptRestart(String, oneshot::Sender<()>),

    SetBreadcrumbState(BreadcrumbRequest),
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

    pub async fn set_breadcrumb_state(&self, state: BreadcrumbRequest) {
        self.sender
            .send(UiCommand::SetBreadcrumbState(state))
            .await
            .unwrap();
    }

    pub async fn set_username(&self, username: Option<String>) {
        self.sender
            .send(UiCommand::SetUsername(username))
            .await
            .unwrap();
    }
}

pub struct SystemState {
    pub net_state: AtomicBool,
    pub qr_processing_pulse: AtomicBool,
    pub qr_test_pulse: AtomicU8,
    pub qr_processing_time_us: AtomicU32,
}

impl SystemState {
    pub fn new() -> Self {
        Self {
            net_state: AtomicBool::new(false),
            qr_processing_pulse: AtomicBool::new(false),
            qr_test_pulse: AtomicU8::new(0),
            qr_processing_time_us: AtomicU32::new(0),
        }
    }
}

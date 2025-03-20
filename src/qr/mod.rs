mod ffi;

use crate::camera::YuvBuffer;
use crate::spmc_buffer;
use anyhow::Context;
use ctru::services::cam::ViewSize;
use ctru::services::svc::handle::BorrowedThread;
use std::fmt::Debug;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::{debug, info};

pub fn is_login_token(token: &str) -> bool {
    token.starts_with("BCT$")
}

#[derive(Debug)]
pub struct ParsedChallenge {
    pub code: String,
    pub secret_code: String,
}

pub fn parse_challenge(challenge: &str) -> Option<ParsedChallenge> {
    let url = url::Url::parse(challenge).ok()?;
    // TODO: check whether the QR code corresponds to our upstream domain?
    let fragment = url.fragment()?;
    let mut split = fragment.split("-");
    let code = split.next()?;
    let secret_code = split.next()?;
    if split.next().is_some() {
        return None;
    }
    // TODO: validate code/secret_code allowed chars?

    Some(ParsedChallenge {
        code: code.to_string(),
        secret_code: secret_code.to_string(),
    })
}

pub struct QrProcessorHandle {
    receiver: async_broadcast::InactiveReceiver<String>,
}

impl QrProcessorHandle {
    pub fn new(receiver: async_broadcast::InactiveReceiver<String>) -> Self {
        Self { receiver }
    }

    async fn scan<T: Debug>(&mut self, mut parse: impl FnMut(String) -> Option<T>) -> T {
        let mut receiver = self.receiver.activate_cloned();
        // drop all accumulated messages, since they are likely stale/from unrelated QRs
        let mut dropped_count = 0;
        while receiver.try_recv().is_ok() {
            dropped_count += 1;
        }
        debug!("dropped {} stale QR codes", dropped_count);

        loop {
            let scanned = match receiver.recv().await {
                Ok(scanned) => scanned,
                Err(async_broadcast::RecvError::Overflowed(_)) => continue,
                Err(async_broadcast::RecvError::Closed) => {
                    panic!("QR processor channel closed")
                }
            };

            // TODO: give some feedback to the user?
            if let Some(parsed) = parse(scanned) {
                self.receiver = receiver.deactivate();

                info!("parsed QR: {:?}", parsed);
                return parsed;
            }
        }
    }

    pub async fn scan_login_token(&mut self) -> String {
        self.scan(|t| is_login_token(&t).then_some(t)).await
    }

    pub async fn scan_challenge(&mut self) -> ParsedChallenge {
        self.scan(|t| parse_challenge(&t)).await
    }
}

fn qr_worker_fn(
    view_size: ViewSize,
    mut output: spmc_buffer::Output<YuvBuffer>,
    sender: async_broadcast::Sender<String>,
    stop_signal: Arc<AtomicBool>,
) {
    let (width, height) = view_size.into();

    let mut context = ffi::Context::new(width as u32, height as u32);
    let mut timings = ffi::ProcessingTimings::default();

    while !stop_signal.load(Ordering::Relaxed) {
        while !output.update() {
            std::thread::sleep(Duration::from_millis(60));
        }

        let buffer = output.output_buffer_mut();
        unsafe { context.set_frame(buffer.as_ptr()) };
        context.process(&mut timings);

        let strings = context.get_strings();
        debug!("Scanned QR: {:?}\n timings = {:?}", strings, timings);
        for s in strings {
            match sender.try_broadcast(s) {
                Ok(_) => {
                    // nothing to do
                }
                Err(async_broadcast::TrySendError::Full(_)) => {
                    unreachable!()
                }
                Err(async_broadcast::TrySendError::Inactive(_)) => {
                    // doesn't matter
                    continue;
                }
                Err(async_broadcast::TrySendError::Closed(_)) => {
                    // we are probably exiting anyways
                    return;
                }
            }
        }
    }
}

pub struct QrWorker {
    stop_signal: Arc<AtomicBool>,
    thread: ManuallyDrop<JoinHandle<()>>,
}

impl QrWorker {
    pub fn new(
        view_size: ViewSize,
        output: spmc_buffer::Output<YuvBuffer>,
        priority: i32,
    ) -> anyhow::Result<(QrWorker, QrProcessorHandle)> {
        let (mut sender, receiver) = async_broadcast::broadcast(8);
        sender.set_overflow(true);
        sender.set_await_active(false);

        let stop_signal = Arc::new(AtomicBool::new(false));

        let thread = std::thread::Builder::new()
            // .priority(prio - 3)
            .stack_size(32 * 1024)
            .name("QrWorker".to_string())
            .spawn({
                let stop_signal = stop_signal.clone();
                move || {
                    BorrowedThread::CURRENT_THREAD
                        .set_thread_priority(priority)
                        .unwrap();

                    qr_worker_fn(view_size, output, sender, stop_signal)
                }
            })
            .context("Failed to spawn camera thread")?;

        Ok((
            Self {
                stop_signal,
                thread: ManuallyDrop::new(thread),
            },
            QrProcessorHandle::new(receiver.deactivate()),
        ))
    }
}

impl Drop for QrWorker {
    fn drop(&mut self) {
        info!("Dropping QrWorker...");

        self.stop_signal.store(true, Ordering::Relaxed);
        info!("Joining QrWorker...");
        unsafe {
            ManuallyDrop::take(&mut self.thread)
                .join()
                .expect("Failed to join qr thread")
        }
    }
}

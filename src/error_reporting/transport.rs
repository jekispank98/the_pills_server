use super::dsn::Dsn;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::Duration;

const QUEUE_CAPACITY: usize = 100;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TARGET: &str = "error_reporting";

pub(super) enum Message {
    Envelope {
        body: Vec<u8>,
        sent: Option<SyncSender<()>>,
    },
    Flush(SyncSender<()>),
}

/// Sends envelopes from a dedicated thread with its own runtime, so reporting
/// works from a panic hook and never blocks request handling.
#[derive(Clone)]
pub struct Transport {
    queue: SyncSender<Message>,
}

impl Transport {
    pub fn start(dsn: Dsn, client_name: String) -> std::io::Result<Self> {
        let (queue, receiver) = sync_channel(QUEUE_CAPACITY);
        thread::Builder::new()
            .name("error-reporter".into())
            .spawn(move || run(receiver, dsn, client_name))?;
        Ok(Self { queue })
    }

    #[cfg(test)]
    pub(super) fn from_queue(queue: SyncSender<Message>) -> Self {
        Self { queue }
    }

    /// Queues an envelope; drops it when the queue is full.
    pub fn send(&self, body: Vec<u8>) {
        match self.queue.try_send(Message::Envelope { body, sent: None }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::warn!(target: TARGET, "error report queue is full, event dropped");
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// Queues an envelope and waits until it has been sent or `timeout` passes.
    pub fn send_and_wait(&self, body: Vec<u8>, timeout: Duration) {
        let (sent, done) = sync_channel(1);
        if self
            .queue
            .try_send(Message::Envelope {
                body,
                sent: Some(sent),
            })
            .is_ok()
        {
            let _ = done.recv_timeout(timeout);
        }
    }

    /// Waits until everything queued so far has been sent, or `timeout` passes.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (sent, done) = sync_channel(1);
        if self.queue.try_send(Message::Flush(sent)).is_err() {
            return false;
        }
        done.recv_timeout(timeout).is_ok()
    }
}

fn run(receiver: Receiver<Message>, dsn: Dsn, client_name: String) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            tracing::warn!(target: TARGET, %error, "error reporting disabled: no runtime");
            return;
        }
    };
    let client = match reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(target: TARGET, %error, "error reporting disabled: no HTTP client");
            return;
        }
    };
    let auth = dsn.auth_header(&client_name);

    for message in receiver {
        match message {
            Message::Envelope { body, sent } => {
                runtime.block_on(post(&client, &dsn, &auth, body));
                if let Some(sent) = sent {
                    let _ = sent.send(());
                }
            }
            Message::Flush(sent) => {
                let _ = sent.send(());
            }
        }
    }
}

async fn post(client: &reqwest::Client, dsn: &Dsn, auth: &str, body: Vec<u8>) {
    let result = client
        .post(dsn.envelope_url().clone())
        .header("content-type", "application/x-sentry-envelope")
        .header("x-sentry-auth", auth)
        .body(body)
        .send()
        .await;
    match result {
        Ok(response) if response.status().is_success() => {}
        Ok(response) => {
            tracing::warn!(target: TARGET, status = %response.status(), "error report rejected");
        }
        Err(error) => {
            tracing::warn!(target: TARGET, %error, "error report not delivered");
        }
    }
}

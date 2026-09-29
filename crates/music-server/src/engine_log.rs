//! The engine's log as the studio follows it: one subscription to ace-server's
//! `GET /logs`, an endless SSE stream, for as long as the service runs. It keeps
//! the lines the engine keeps, for the log view, and the running job's
//! progress, which windows follow as server-sent events. A window never opens
//! the engine's stream itself: while the engine renders the stream does not go
//! quiet, so a request reading it would hold a connection the window needs.

use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::response::sse::Event;
use eventsource_stream::Eventsource;
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{Stream, StreamExt};

use crate::progress::{self, Progress};

/// As many lines as ace-server keeps in its ring (`LOG_RING_SIZE`).
const LINES: usize = 512;
/// The wait before subscribing again to an engine that is down or restarting.
const RECONNECT: Duration = Duration::from_secs(1);
/// The most often a window hears of progress: counter lines come several times
/// a second, and a card needs no more than this.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub struct EngineLog {
    /// `None` while there is no subscription: the engine is starting or down.
    lines: Arc<Mutex<Option<VecDeque<String>>>>,
    progress: Arc<watch::Sender<Option<Progress>>>,
}

impl EngineLog {
    /// Follows the engine log at `url` from now on.
    pub fn follow(http: reqwest::Client, url: String) -> Self {
        let log = Self { lines: Arc::default(), progress: Arc::new(watch::Sender::new(None)) };
        tokio::spawn(log.clone().subscribe(http, url));
        log
    }

    /// The engine's recent lines, or `None` while there is no subscription.
    pub fn lines(&self) -> Option<Vec<String>> {
        self.lines.lock().expect("engine log").as_ref().map(|lines| lines.iter().cloned().collect())
    }

    /// The running job's progress as server-sent events: the current value
    /// first, then its changes.
    pub fn progress_events(&self) -> impl Stream<Item = Result<Event, Infallible>> + use<> {
        WatchStream::new(self.progress.subscribe())
            .throttle(PROGRESS_EVERY)
            .map(|progress| Ok(Event::default().json_data(progress).expect("progress is plain data")))
    }

    async fn subscribe(self, http: reqwest::Client, url: String) {
        loop {
            if let Ok(response) = http.get(&url).send().await.and_then(reqwest::Response::error_for_status) {
                // every subscriber is sent the engine's whole ring first
                self.reset(Some(VecDeque::with_capacity(LINES)));
                let mut events = response.bytes_stream().eventsource();
                while let Some(Ok(event)) = events.next().await {
                    self.push(event.data.trim());
                }
            }
            self.reset(None);
            tokio::time::sleep(RECONNECT).await;
        }
    }

    fn reset(&self, lines: Option<VecDeque<String>>) {
        *self.lines.lock().expect("engine log") = lines;
        self.progress.send_replace(None);
    }

    fn push(&self, line: &str) {
        if line.is_empty() {
            return;
        }
        if let Some(lines) = self.lines.lock().expect("engine log").as_mut() {
            if lines.len() == LINES {
                lines.pop_front();
            }
            lines.push_back(line.to_owned());
        }
        self.progress.send_if_modified(|current| {
            let next = progress::step(current.clone(), line);
            let changed = *current != next;
            *current = next;
            changed
        });
    }
}

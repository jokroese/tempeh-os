use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse};
use axum::{Json, Router, routing::get};
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const LIVE_RING_CAPACITY: usize = 10_800;
const LIVE_CONTROL_HTML: &str = include_str!("live_control.html");

#[derive(Debug, Clone, Serialize)]
struct LiveSample {
    seq: u64,
    time_s: f32,
    room_air_temp_c: Option<f32>,
    box_air_temp_c: f32,
    product_temp_c: Option<f32>,
    heater_on: bool,
    reason: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct LiveStatus {
    csv_path: String,
    sample_count: usize,
    retained_sample_count: usize,
    first_retained_seq: Option<u64>,
    latest: Option<LiveSample>,
}

#[derive(Debug)]
struct LiveRunState {
    csv_path: String,
    samples: VecDeque<LiveSample>,
    next_seq: u64,
}

impl LiveRunState {
    fn new(csv_path: impl Into<String>) -> Self {
        Self {
            csv_path: csv_path.into(),
            samples: VecDeque::with_capacity(LIVE_RING_CAPACITY),
            next_seq: 1,
        }
    }

    fn push(
        &mut self,
        time_s: f32,
        room_air_temp_c: Option<f32>,
        box_air_temp_c: f32,
        product_temp_c: Option<f32>,
        heater_on: bool,
        reason: &'static str,
    ) -> LiveSample {
        let sample = LiveSample {
            seq: self.next_seq,
            time_s,
            room_air_temp_c,
            box_air_temp_c,
            product_temp_c,
            heater_on,
            reason,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        if self.samples.len() == LIVE_RING_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(sample.clone());
        sample
    }

    fn samples_after(&self, after: u64) -> Vec<LiveSample> {
        self.samples
            .iter()
            .filter(|sample| sample.seq > after)
            .cloned()
            .collect()
    }

    fn status(&self) -> LiveStatus {
        LiveStatus {
            csv_path: self.csv_path.clone(),
            sample_count: self.next_seq.saturating_sub(1) as usize,
            retained_sample_count: self.samples.len(),
            first_retained_seq: self.samples.front().map(|sample| sample.seq),
            latest: self.samples.back().cloned(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct LiveAppState {
    run: Mutex<LiveRunState>,
    events: broadcast::Sender<LiveSample>,
}

impl LiveAppState {
    pub(crate) fn new(csv_path: impl Into<String>) -> Self {
        let (events, _receiver) = broadcast::channel(1_024);
        Self {
            run: Mutex::new(LiveRunState::new(csv_path)),
            events,
        }
    }

    pub(crate) fn push_sample(
        &self,
        time_s: f32,
        room_air_temp_c: Option<f32>,
        box_air_temp_c: f32,
        product_temp_c: Option<f32>,
        heater_on: bool,
        reason: &'static str,
    ) {
        let sample = {
            let mut run = self.run.lock().expect("live state mutex poisoned");
            run.push(
                time_s,
                room_air_temp_c,
                box_air_temp_c,
                product_temp_c,
                heater_on,
                reason,
            )
        };
        let _ = self.events.send(sample);
    }
}

pub(crate) type SharedLiveAppState = Arc<LiveAppState>;

#[derive(Debug, Deserialize)]
struct SamplesQuery {
    after: Option<u64>,
}

async fn live_index() -> Html<&'static str> {
    Html(LIVE_CONTROL_HTML)
}

async fn live_status(State(state): State<SharedLiveAppState>) -> impl IntoResponse {
    let run = state.run.lock().expect("live state mutex poisoned");
    Json(run.status())
}

async fn live_samples(
    State(state): State<SharedLiveAppState>,
    Query(query): Query<SamplesQuery>,
) -> impl IntoResponse {
    let after = query.after.unwrap_or(0);
    let run = state.run.lock().expect("live state mutex poisoned");
    Json(run.samples_after(after))
}

async fn live_events(
    State(state): State<SharedLiveAppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(sample) => {
                    let Ok(json) = serde_json::to_string(&sample) else {
                        continue;
                    };
                    yield Ok(Event::default()
                        .event("sample")
                        .id(sample.seq.to_string())
                        .data(json));
                }
                Err(broadcast::error::RecvError::Lagged(_missed)) => {
                    yield Ok(Event::default().event("resync").data("lagged"));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}

pub(crate) fn spawn_live_server(
    state: SharedLiveAppState,
    addr: SocketAddr,
) -> thread::JoinHandle<Result<(), String>> {
    thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|error| format!("failed to create Tokio runtime: {error}"))?;
        runtime.block_on(async move {
            let app = Router::new()
                .route("/", get(live_index))
                .route("/api/status", get(live_status))
                .route("/api/samples", get(live_samples))
                .route("/events", get(live_events))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|error| format!("failed to bind live UI at http://{addr}: {error}"))?;
            axum::serve(listener, app)
                .await
                .map_err(|error| format!("live UI server failed: {error}"))
        })
    })
}

//! Lokale JSON-Lines-Statusausgabe für Desktop-Bedienung; keine neue Effekt-API.
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use arcs::core::Artifact;
use serde_json::{Value, json};

use crate::task_runtime::{RuntimeConfig, TaskRuntime};
use crate::{Host, HostError};

fn emit(writer: &mut impl Write, value: Value) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, &value).map_err(|e| e.to_string())?;
    writer.write_all(b"\n").map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())
}
fn fail(error: HostError) -> String {
    error.to_string()
}
fn summary(artifact: Artifact) -> Value {
    json!({"version_id":artifact.version_id,"schema_id":artifact.schema_id,
        "producer_id":artifact.created_by.id,"created_at":artifact.created_at})
}

/// Stop wird zwischen den begrenzten Host-Runden und während der Pausen
/// geprüft. Laufende Adapterdialoge behalten ihre bisherigen Deadlines.
pub fn run(
    mut host: Host,
    runtime: Option<RuntimeConfig>,
    stop_file: &Path,
    max_ticks: Option<u64>,
    writer: &mut impl Write,
) -> Result<(), String> {
    let interval = runtime
        .as_ref()
        .map(|config| config.interval_ms)
        .unwrap_or(250);
    let mut runtime = runtime
        .map(|config| TaskRuntime::open(&host, config))
        .transpose()
        .map_err(fail)?;
    let started = Instant::now();
    emit(
        writer,
        json!({"event":"ready","catalog":host.adapter_catalog(),"has_runtime":runtime.is_some()}),
    )?;
    let outcome = (|| {
        if runtime.is_none() && !stop_file.exists() {
            host.run_configured_operations().map_err(fail)?;
        }
        let (mut ticks, mut cursor) = (
            0,
            host.store()
                .latest_sequence()
                .map_err(|error| format!("{error:?}"))?
                .saturating_sub(20),
        );
        let mut last_snapshot = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        while !stop_file.exists() && max_ticks.is_none_or(|max| ticks < max) {
            let completed = if let Some(runtime) = &mut runtime {
                runtime
                    .tick(&mut host)
                    .map_err(fail)?
                    .ready_for_review
                    .len()
            } else {
                host.poll_event_tick().map_err(fail)?;
                0
            };
            ticks += 1;
            if last_snapshot.elapsed() >= Duration::from_secs(1)
                || completed > 0
                || max_ticks == Some(ticks)
            {
                snapshot(&host, runtime.as_ref(), ticks, &mut cursor, started, writer)?;
                last_snapshot = Instant::now();
            }
            let until = Instant::now() + Duration::from_millis(interval);
            while Instant::now() < until && !stop_file.exists() {
                thread::sleep(
                    until
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(25)),
                );
            }
        }
        snapshot(&host, runtime.as_ref(), ticks, &mut cursor, started, writer)?;
        Ok(())
    })();
    let shutdown = host.shutdown().map_err(fail);
    match outcome.and(shutdown) {
        Ok(()) => emit(writer, json!({"event":"stopped"})),
        Err(error) => {
            let _ = emit(writer, json!({"event":"error","message":error}));
            Err(error)
        }
    }
}
fn snapshot(
    host: &Host,
    runtime: Option<&TaskRuntime>,
    ticks: u64,
    cursor: &mut u64,
    started: Instant,
    writer: &mut impl Write,
) -> Result<(), String> {
    let report = host.report().map_err(fail)?;
    let page = host
        .store()
        .committed_page_after(*cursor, 50)
        .map_err(|e| format!("{e:?}"))?;
    let mut recent = Vec::new();
    for event in page {
        *cursor = event.sequence;
        recent.push(summary(event.artifact));
    }
    let adapters=report.adapters.into_iter().map(|adapter|json!({"adapter_id":adapter.adapter_id,
        "observations_received":adapter.observations_received,"observations_committed":adapter.observations_committed,
        "requests":adapter.requests.len(),"transforms":adapter.transforms.len()})).collect::<Vec<_>>();
    let runtime = runtime
        .map(|runtime| runtime.report(ticks))
        .transpose()
        .map_err(fail)?;
    emit(
        writer,
        json!({"event":"snapshot","ticks":ticks,"uptime_ms":started.elapsed().as_millis(),
        "stored_artifacts":report.stored_artifacts,"world_revision":report.world_revision,
        "adapters":adapters,"runtime":runtime,"recent_artifacts":recent}),
    )
}

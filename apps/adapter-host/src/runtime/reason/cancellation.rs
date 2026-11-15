//! Aufgegebene Antworten verwerfen, ohne fremde Adapter-Sessions abzubrechen.
use std::collections::HashSet;
use std::sync::mpsc::TryRecvError;
use std::time::Instant;

use crate::Host;

impl Host {
    pub(crate) fn pending_reasoning_request_ids(&self) -> Vec<String> {
        self.reasoning_jobs
            .values()
            .map(|job| job.prepared.invocation().request_id.clone())
            .collect()
    }
    /// Erst nach dem dauerhaften Abbruch der beauftragenden Runtime aufrufen.
    /// Der Worker behält bis Antwort oder Deadline seine Ressourcenreservierung.
    pub(crate) fn discard_reasoning_requests(&mut self, requests: &HashSet<String>) {
        for job in self.reasoning_jobs.values_mut() {
            if requests.contains(&job.prepared.invocation().request_id) {
                job.discarded = true;
            }
        }
        self.reap_discarded_reasoning();
    }

    pub(crate) fn reap_discarded_reasoning(&mut self) {
        let mut finished = Vec::new();
        for (key, job) in &self.reasoning_jobs {
            if !job.discarded {
                continue;
            }
            let expired = Instant::now() >= job.deadline;
            let abort = match job.response.try_recv() {
                Ok(Ok(_)) => false,
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => true,
                Err(TryRecvError::Empty) if expired => true,
                Err(TryRecvError::Empty) => continue,
            };
            if abort {
                for adapter in &self.adapters {
                    if job.resources.contains(&adapter.config.grant.adapter_id) {
                        adapter.endpoint.abort();
                    }
                }
                self.unavailable_adapters
                    .extend(job.resources.iter().cloned());
            }
            finished.push(key.clone());
        }
        for key in finished {
            let job = self
                .reasoning_jobs
                .remove(&key)
                .expect("discarded job exists");
            // Workers never commit Core results. A late response is deliberately
            // not passed to GeneralReasoningService::finish; its outcome remains
            // unknown, just like a lost response, rather than a fabricated success.
            if job.worker.is_finished() {
                let _ = job.worker.join();
            }
        }
    }
}

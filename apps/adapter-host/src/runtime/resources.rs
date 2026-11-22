//! Admission is atomic under the Host's mutable borrow. Live jobs own usage;
//! there is no second reservation ledger that could drift after cancellation.
use crate::{Host, HostError};
use arcs::adapters::AdapterId;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub(super) type ResourceClaims = BTreeMap<String, u64>;

#[derive(Debug, Clone, Serialize)]
pub struct ResourcePoolReport {
    pub name: String,
    pub capacity: u64,
    pub used: u64,
    pub available: u64,
}

impl Host {
    pub(super) fn reasoning_claims(
        &self,
        adapters: &HashSet<AdapterId>,
    ) -> Result<ResourceClaims, HostError> {
        let mut claims = ResourceClaims::new();
        for adapter in &self.adapters {
            if adapters.contains(&adapter.config.grant.adapter_id) {
                for (resource, units) in &adapter.config.reasoning_resources {
                    *claims.entry(resource.clone()).or_default() += u64::from(*units);
                }
            }
        }
        for (resource, units) in &claims {
            if *units > u64::from(self.resource_pools[resource]) {
                return Err(HostError::InvalidConfig(format!(
                    "combined reasoning operation needs {units} units of {resource}, exceeding its configured capacity"
                )));
            }
        }
        Ok(claims)
    }

    fn used_resources(&self) -> ResourceClaims {
        let mut used = ResourceClaims::new();
        for job in self.reasoning_jobs.values() {
            for (resource, units) in &job.claims {
                *used.entry(resource.clone()).or_default() += units;
            }
        }
        for job in self.transform_jobs.values() {
            for (resource, units) in &job.claims {
                *used.entry(resource.clone()).or_default() += units;
            }
        }
        for job in self.reconnect_jobs.values() {
            for (resource, units) in &job.claims {
                *used.entry(resource.clone()).or_default() += units;
            }
        }
        used
    }

    pub(super) fn ensure_resources_available(
        &self,
        claims: &ResourceClaims,
    ) -> Result<(), HostError> {
        let used = self.used_resources();
        for (resource, units) in claims {
            let available =
                u64::from(self.resource_pools[resource]) - used.get(resource).copied().unwrap_or(0);
            if *units > available {
                return Err(HostError::ResourceBusy {
                    resource: resource.clone(),
                    requested: *units,
                    available,
                });
            }
        }
        Ok(())
    }

    pub fn resource_usage(&self) -> Vec<ResourcePoolReport> {
        let used = self.used_resources();
        self.resource_pools
            .iter()
            .map(|(name, capacity)| {
                let capacity = u64::from(*capacity);
                let used = used.get(name).copied().unwrap_or(0);
                ResourcePoolReport {
                    name: name.clone(),
                    capacity,
                    used,
                    available: capacity - used,
                }
            })
            .collect()
    }
}

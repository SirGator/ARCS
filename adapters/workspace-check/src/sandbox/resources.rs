//! Per-check limits in an explicitly delegated cgroup v2 subtree.
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

mod ownership;

use ownership::GroupLease;

const CPU_PERIOD_US: u64 = 100_000;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResourceLimits {
    pub parent: PathBuf,
    pub cpu_percent: u32,
    pub memory_bytes: u64,
    pub max_tasks: u32,
}

#[derive(Serialize)]
pub(crate) struct ResourceUsage {
    pub cpu_percent: u32,
    pub memory_bytes: u64,
    pub max_tasks: u32,
    pub cpu_throttled_periods: u64,
    pub memory_oom_events: u64,
    pub memory_oom_kills: u64,
    pub task_limit_events: u64,
}

impl ResourceUsage {
    pub(crate) fn hit_limit(&self) -> bool {
        self.memory_oom_events > 0 || self.memory_oom_kills > 0 || self.task_limit_events > 0
    }
}

pub(super) struct ResourceGroup<'a> {
    lease: GroupLease<'a>,
    limits: &'a ResourceLimits,
}

impl ResourceLimits {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.cpu_percent == 0 || self.memory_bytes == 0 || self.max_tasks == 0 {
            return Err("isolated CPU, memory and task limits must be positive".into());
        }
        let enabled = fs::read_to_string(self.parent.join("cgroup.subtree_control"))
            .map_err(|error| format!("delegated cgroup v2 parent is unavailable: {error}"))?;
        if ["cpu", "memory", "pids"].iter().any(|controller| {
            !enabled
                .split_ascii_whitespace()
                .any(|word| word == *controller)
        }) {
            return Err(
                "cgroup v2 parent must delegate the cpu, memory and pids controllers".into(),
            );
        }
        Ok(())
    }

    pub(super) fn recover(&self, journal: &Connection) -> io::Result<()> {
        ownership::recover(&self.parent, journal)
    }

    pub(super) fn create<'a>(
        &'a self,
        journal: &'a Connection,
        invocation: &'a str,
    ) -> io::Result<ResourceGroup<'a>> {
        let mut lease = GroupLease::create(&self.parent, journal, invocation)?;
        let quota = u64::from(self.cpu_percent) * (CPU_PERIOD_US / 100);
        write_control(
            &lease.directory,
            "cpu.max",
            &format!("{quota} {CPU_PERIOD_US}"),
        )?;
        write_control(
            &lease.directory,
            "memory.max",
            &self.memory_bytes.to_string(),
        )?;
        write_control(&lease.directory, "memory.swap.max", "0")?;
        write_control(&lease.directory, "memory.oom.group", "1")?;
        write_control(&lease.directory, "pids.max", &self.max_tasks.to_string())?;
        OpenOptions::new()
            .write(true)
            .open(lease.directory.join("cgroup.kill"))?;
        lease.confirm()?;
        Ok(ResourceGroup {
            lease,
            limits: self,
        })
    }
}

impl ResourceGroup<'_> {
    pub(super) fn membership(&self) -> PathBuf {
        self.lease.directory.join("cgroup.procs")
    }

    pub(super) fn finish(self) -> io::Result<ResourceUsage> {
        self.lease.stop()?;
        let memory = fs::read_to_string(self.lease.directory.join("memory.events"))?;
        let tasks = fs::read_to_string(self.lease.directory.join("pids.events"))?;
        let cpu = fs::read_to_string(self.lease.directory.join("cpu.stat"))?;
        let usage = ResourceUsage {
            cpu_percent: self.limits.cpu_percent,
            memory_bytes: self.limits.memory_bytes,
            max_tasks: self.limits.max_tasks,
            cpu_throttled_periods: counter(&cpu, "nr_throttled")?,
            memory_oom_events: counter(&memory, "oom")?,
            memory_oom_kills: counter(&memory, "oom_kill")?,
            task_limit_events: counter(&tasks, "max")?,
        };
        self.lease.remove()?;
        Ok(usage)
    }
}

fn write_control(directory: &Path, name: &str, value: &str) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open(directory.join(name))
        .and_then(|mut file| file.write_all(value.as_bytes()))
        .map_err(|error| io::Error::new(error.kind(), format!("write {name}: {error}")))
}

fn counter(report: &str, key: &str) -> io::Result<u64> {
    report
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(' ')?;
            (name == key).then_some(value)
        })
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing or invalid cgroup counter {key}"),
            )
        })
}

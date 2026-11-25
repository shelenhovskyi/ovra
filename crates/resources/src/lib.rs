//! Memory and CPU of process trees and of the computer, through `sysinfo`
//! (`docs/specs/agent-resources.md`). Implements
//! [`ResourceMonitor`](ovra_core::services::ResourceMonitor).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use ovra_core::protocol::resources::{SystemLoad, Usage};
use ovra_core::services::{ResourceMonitor, ResourceSample};
use sysinfo::{MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// Samples with one `System` kept between calls: CPU use is measured
/// against the previous call.
#[derive(Debug)]
pub struct SysResources {
    system: Mutex<System>,
}

impl SysResources {
    pub fn new() -> Self {
        Self {
            system: Mutex::new(System::new()),
        }
    }
}

impl Default for SysResources {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceMonitor for SysResources {
    fn sample(&self, roots: &[u32]) -> ResourceSample {
        let mut system = self.system.lock().unwrap_or_else(PoisonError::into_inner);
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        system.refresh_cpu_usage();
        // Threads are listed as processes on Linux; they share their
        // process's memory, so they are left out.
        let kind = ProcessRefreshKind::nothing()
            .with_memory()
            .with_cpu()
            .without_tasks();
        system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
        let processes: Vec<Proc> = system
            .processes()
            .values()
            .filter(|p| p.thread_kind().is_none())
            .map(|p| Proc {
                pid: p.pid().as_u32(),
                parent: p.parent().map(Pid::as_u32),
                memory: p.memory(),
                cpu: p.cpu_usage(),
            })
            .collect();
        ResourceSample {
            system: SystemLoad {
                memory_total_bytes: system.total_memory(),
                memory_available_bytes: system.available_memory(),
                cpu_percent: percent(system.global_cpu_usage()),
            },
            trees: trees(&processes, roots),
        }
    }
}

/// One process as sampled.
#[derive(Debug, Clone, Copy)]
struct Proc {
    pid: u32,
    parent: Option<u32>,
    memory: u64,
    /// 100 is one core.
    cpu: f32,
}

/// The usage of each root's tree: the root and every descendant; `None`
/// for a root that is not running.
fn trees(processes: &[Proc], roots: &[u32]) -> Vec<Option<Usage>> {
    let by_pid: BTreeMap<u32, &Proc> = processes.iter().map(|p| (p.pid, p)).collect();
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for p in processes {
        if let Some(parent) = p.parent.filter(|&parent| parent != p.pid) {
            children.entry(parent).or_default().push(p.pid);
        }
    }
    roots
        .iter()
        .map(|root| {
            by_pid.get(root)?;
            let mut seen = BTreeSet::new();
            let mut next = vec![*root];
            let (mut memory, mut cpu) = (0_u64, 0_f32);
            while let Some(pid) = next.pop() {
                if !seen.insert(pid) {
                    continue;
                }
                if let Some(p) = by_pid.get(&pid) {
                    memory = memory.saturating_add(p.memory);
                    cpu += p.cpu;
                }
                next.extend(children.get(&pid).into_iter().flatten());
            }
            Some(Usage {
                memory_bytes: memory,
                cpu_percent: percent(cpu),
                processes: u32::try_from(seen.len()).unwrap_or(u32::MAX),
            })
        })
        .collect()
}

/// A share as a whole percent; nothing measured yet counts as 0.
fn percent(value: f32) -> u32 {
    if value.is_finite() && value > 0.0 {
        // In range: a few thousand percent at most (100 per core).
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rounded = value.round().min(u32::MAX as f32) as u32;
        rounded
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: u32, parent: Option<u32>, memory: u64, cpu: f32) -> Proc {
        Proc {
            pid,
            parent,
            memory,
            cpu,
        }
    }

    #[test]
    fn a_tree_is_its_root_and_every_descendant() {
        let processes = [
            proc(1, None, 10, 0.0),
            // The agent, a command it runs and that command's child.
            proc(100, Some(1), 300, 20.4),
            proc(101, Some(100), 50, 80.0),
            proc(102, Some(101), 5, 0.0),
            // Someone else's.
            proc(200, Some(1), 999, 99.0),
            // A loop in the parents does not hang.
            proc(300, Some(301), 1, 0.0),
            proc(301, Some(300), 1, 0.0),
        ];
        let usage = trees(&processes, &[100, 101, 404, 300]);
        let at = |memory_bytes, cpu_percent, processes| {
            Some(Usage {
                memory_bytes,
                cpu_percent,
                processes,
            })
        };
        assert_eq!(usage, [at(355, 100, 3), at(55, 80, 2), None, at(2, 0, 2)]);
    }

    #[test]
    fn percents_round_and_never_go_below_zero() {
        assert_eq!(percent(12.5), 13);
        assert_eq!(percent(-1.0), 0);
        assert_eq!(percent(f32::NAN), 0);
        assert_eq!(percent(250.2), 250);
    }

    #[test]
    fn this_process_is_found_with_its_memory() {
        let monitor = SysResources::new();
        let me = std::process::id();
        let sample = monitor.sample(&[me, u32::MAX]);
        assert!(sample.system.memory_total_bytes > 0);
        assert!(sample.system.memory_available_bytes <= sample.system.memory_total_bytes);
        let [Some(mine), None] = sample.trees[..] else {
            panic!("unexpected {:?}", sample.trees);
        };
        assert!(mine.memory_bytes > 0);
        assert!(mine.processes >= 1);
    }
}

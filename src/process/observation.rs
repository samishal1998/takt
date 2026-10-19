//! Bounded, best-effort descendant observation for hosts without owned cgroups.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;

const MAX_PROCESSES: usize = 131072;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    pid: i32,
    started: (u64, u64),
}

#[derive(Clone, Copy, Debug)]
struct Process {
    identity: Identity,
    parent: i32,
    group: i32,
    live: bool,
    #[cfg(target_os = "linux")]
    resident_bytes: Option<u64>,
}

pub(crate) struct ProcessTable {
    processes: BTreeMap<i32, Process>,
    children: BTreeMap<i32, Vec<i32>>,
    unreadable: BTreeSet<i32>,
}

impl ProcessTable {
    pub(crate) fn capture() -> io::Result<Self> {
        let mut table = Self {
            processes: BTreeMap::new(),
            children: BTreeMap::new(),
            unreadable: BTreeSet::new(),
        };
        for pid in process_ids()? {
            match read_process(pid) {
                Ok(Some(process)) => {
                    table.children.entry(process.parent).or_default().push(pid);
                    table.processes.insert(pid, process);
                }
                Ok(None) => {}
                Err(_) => {
                    // An inaccessible unrelated process does not invalidate every
                    // task's sample. An already-owned identity remains unresolved.
                    table.unreadable.insert(pid);
                }
            }
        }
        Ok(table)
    }
}

struct Tracked {
    identity: Identity,
    signalled: Option<i32>,
}

#[derive(Default)]
pub(super) struct ProcessTree {
    tracked: BTreeMap<i32, Tracked>,
}

impl ProcessTree {
    fn observe(&mut self, table: &ProcessTable, leader: i32, group: i32) {
        self.tracked.retain(|pid, tracked| {
            table.unreadable.contains(pid)
                || table
                    .processes
                    .get(pid)
                    .is_some_and(|p| p.identity == tracked.identity)
        });
        for (&pid, process) in &table.processes {
            // The unreaped leader reserves this PID/PGID until cleanup finishes.
            // Original-group members cover children orphaned between samples.
            if pid == leader || process.group == group {
                self.tracked.entry(pid).or_insert(Tracked {
                    identity: process.identity,
                    signalled: None,
                });
            }
        }
        let mut queue: VecDeque<_> = self.tracked.keys().copied().collect();
        while let Some(parent) = queue.pop_front() {
            let Some(parent_process) = table.processes.get(&parent) else {
                continue;
            };
            if self.tracked[&parent].identity != parent_process.identity {
                continue;
            }
            for child in table.children.get(&parent).into_iter().flatten() {
                let process = table.processes[child];
                // Linux start ticks are monotonic. Reject an old PPID edge
                // observed across PID reuse. Darwin uses wall time, which can
                // move backwards between a parent's birth and its child's.
                #[cfg(target_os = "linux")]
                if process.identity.started < parent_process.identity.started {
                    continue;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    self.tracked.entry(*child)
                {
                    entry.insert(Tracked {
                        identity: process.identity,
                        signalled: None,
                    });
                    queue.push_back(*child);
                }
            }
        }
    }

    pub(super) fn memory(&mut self, table: &ProcessTable, leader: i32, group: i32) -> Option<u64> {
        self.observe(table, leader, group);
        if table.unreadable.contains(&leader) {
            return None;
        }
        self.tracked.keys().try_fold(0u64, |total, pid| {
            let process = table.processes.get(pid)?;
            let bytes = if process.live {
                resident_memory(process)?
            } else {
                0
            };
            total.checked_add(bytes)
        })
    }

    pub(super) fn refresh(&mut self, leader: i32, group: i32) -> io::Result<bool> {
        let table = ProcessTable::capture()?;
        self.observe(&table, leader, group);
        if table.unreadable.contains(&leader)
            || self
                .tracked
                .keys()
                .any(|pid| table.unreadable.contains(pid))
        {
            return Err(io::Error::other("an owned process could not be observed"));
        }
        Ok(self.tracked.keys().any(|pid| table.processes[pid].live))
    }

    /// Signal positively identified descendants not covered by an earlier signal.
    /// Rechecking start time rejects stale PIDs; check/kill is still not atomic.
    pub(super) fn signal(
        &mut self,
        group: i32,
        signal: i32,
        group_signalled: bool,
    ) -> io::Result<()> {
        let mut first_error = None;
        for tracked in self.tracked.values_mut() {
            if tracked.signalled == Some(signal) {
                continue;
            }
            let result = (|| {
                let Some(process) = read_process(tracked.identity.pid)? else {
                    return Ok(());
                };
                if process.identity != tracked.identity || !process.live {
                    return Ok(());
                }
                if group_signalled && process.group == group {
                    tracked.signalled = Some(signal);
                    return Ok(());
                }
                // SAFETY: this positive PID was just revalidated against the
                // observed descendant's start identity. No unverified group ID
                // is signalled. Platforms without PID handles retain a small race.
                if unsafe { libc::kill(process.identity.pid, signal) } == -1 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(error);
                    }
                }
                tracked.signalled = Some(signal);
                Ok(())
            })();
            if first_error.is_none() {
                first_error = result.err();
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(target_os = "linux")]
fn process_ids() -> io::Result<Vec<i32>> {
    let mut pids = Vec::new();
    for (index, entry) in std::fs::read_dir("/proc")?.enumerate() {
        if index >= MAX_PROCESSES {
            return Err(io::Error::other(
                "process observation exceeds 131072 entries",
            ));
        }
        if let Some(pid) = entry?
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
            && pid > 0
        {
            pids.push(pid);
        }
    }
    Ok(pids)
}

#[cfg(target_os = "linux")]
fn read_process(pid: i32) -> io::Result<Option<Process>> {
    use std::io::Read;
    let file = match std::fs::File::open(format!("/proc/{pid}/stat")) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut stat = String::new();
    file.take(8193).read_to_string(&mut stat)?;
    if stat.len() > 8192 {
        return Err(io::Error::other("process stat exceeds observation bound"));
    }
    // SAFETY: sysconf takes no pointers; nonpositive results mean unknown.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    parse_proc_stat(pid, &stat, page_size)
        .map(Some)
        .ok_or_else(|| io::Error::other("invalid process stat"))
}

#[cfg(target_os = "linux")]
fn parse_proc_stat(pid: i32, stat: &str, page_size: i64) -> Option<Process> {
    // comm can contain spaces, parentheses and newlines; numeric fields cannot.
    let (_, tail) = stat.rsplit_once(')')?;
    let mut fields = tail.split_whitespace();
    let state = fields.next()?.as_bytes().first().copied()?;
    let parent = fields.next()?.parse().ok()?; // field 4
    let group = fields.next()?.parse().ok()?; // field 5
    let started = fields.nth(16)?.parse().ok()?; // field 22
    fields.next()?; // virtual bytes, field 23
    let rss = fields.next()?.parse::<i64>().ok()?; // field 24
    Some(Process {
        identity: Identity {
            pid,
            started: (started, 0),
        },
        parent,
        group,
        live: !matches!(state, b'Z' | b'X'),
        resident_bytes: u64::try_from(rss).ok().and_then(|rss| {
            (page_size > 0)
                .then_some(page_size as u64)
                .and_then(|size| rss.checked_mul(size))
        }),
    })
}

#[cfg(target_os = "linux")]
fn resident_memory(process: &Process) -> Option<u64> {
    process.resident_bytes
}

#[cfg(target_os = "macos")]
fn process_ids() -> io::Result<Vec<i32>> {
    const PROC_ALL_PIDS: u32 = 1;
    let mut pids = vec![0i32; MAX_PROCESSES];
    // SAFETY: writable array and exact byte length for the libproc PID listing.
    let bytes = unsafe {
        libc::proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr().cast(),
            std::mem::size_of_val(pids.as_slice()) as i32,
        )
    };
    // libproc returns zero on error. An empty host cannot contain this observer.
    if bytes <= 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes = bytes as usize;
    if !bytes.is_multiple_of(std::mem::size_of::<i32>())
        || bytes >= std::mem::size_of_val(pids.as_slice())
    {
        return Err(io::Error::other(
            "process observation exceeds bound or has invalid length",
        ));
    }
    pids.truncate(bytes / std::mem::size_of::<i32>());
    pids.retain(|pid| *pid > 0);
    Ok(pids)
}

#[cfg(target_os = "macos")]
fn read_process(pid: i32) -> io::Result<Option<Process>> {
    // SAFETY: proc_bsdinfo is a C output structure supporting zero initialization.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    // SAFETY: exact output buffer for BSDINFO. arg=1 includes zombies, so an
    // unreaped leader remains distinguishable from an inaccessible process.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            1,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            std::mem::size_of_val(&info) as i32,
        )
    };
    if read <= 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ESRCH) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if read as usize != std::mem::size_of_val(&info) || info.pbi_pid != pid as u32 {
        return Err(io::Error::other("invalid process BSD info"));
    }
    Ok(Some(Process {
        identity: Identity {
            pid,
            started: (info.pbi_start_tvsec, info.pbi_start_tvusec),
        },
        parent: info.pbi_ppid as i32,
        group: info.pbi_pgid as i32,
        live: info.pbi_status != 5, // SZOMB
    }))
}

#[cfg(target_os = "macos")]
fn resident_memory(process: &Process) -> Option<u64> {
    // Query RSS only for owned processes, then verify that it still belongs to
    // the enumerated identity. Do not charge a recycled PID to the old task.
    let current = read_process(process.identity.pid).ok()??;
    if current.identity != process.identity {
        return None;
    }
    if !current.live {
        return Some(0);
    }
    // SAFETY: proc_taskinfo is a C output structure supporting zero initialization.
    let mut task: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    // SAFETY: exact writable buffer for the requested TASKINFO flavor.
    let read = unsafe {
        libc::proc_pidinfo(
            process.identity.pid,
            libc::PROC_PIDTASKINFO,
            0,
            (&mut task as *mut libc::proc_taskinfo).cast(),
            std::mem::size_of_val(&task) as i32,
        )
    };
    if read as usize != std::mem::size_of_val(&task)
        || read_process(process.identity.pid).ok()??.identity != process.identity
    {
        return None;
    }
    Some(task.pti_resident_size)
}

#[cfg(test)]
mod tests;

//! Native discovery and owned cgroup-v2 protection. Discovery never writes OS state.
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct ResourceSnapshot {
    pub cpu_millis: Option<u64>,
    pub memory_available: Option<u64>,
    pub provenance: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnforcementMode {
    Auto,
    Required,
    Off,
}

#[derive(Clone, Debug, Serialize)]
pub struct Capabilities {
    pub cpu: String,
    pub aggregate_memory: String,
    pub per_task_memory: String,
    pub process_tree_containment: String,
    pub descendant_accounting: String,
    pub swap: String,
    pub details: Vec<String>,
}

impl Capabilities {
    fn fallback(mode: EnforcementMode) -> Self {
        let memory = if mode == EnforcementMode::Off {
            "observation_only"
        } else {
            "best_effort_watchdog"
        };
        Self {
            cpu: "admission_only".into(),
            aggregate_memory: memory.into(),
            per_task_memory: memory.into(),
            process_tree_containment: "process_group".into(),
            descendant_accounting: "best_effort_process_group_rss".into(),
            swap: "external_policy_only".into(),
            details: vec!["Process groups cannot contain descendants that change session/group. Sampled RSS may double-count shared pages and miss short-lived or detached processes.".into()],
        }
    }
}

/// No directories are created by this capability inspection.
pub fn inspect_capabilities() -> Capabilities {
    let mut caps = Capabilities::fallback(EnforcementMode::Auto);
    #[cfg(target_os = "linux")]
    match cgroup_location() {
        Ok(location) => caps.details.push(format!(
            "cgroup v2 detected at {}; controller permissions are verified only when preparing a run",
            location.current.display()
        )),
        Err(error) => caps.details.push(format!("cgroup v2 unavailable: {error}")),
    }
    #[cfg(target_os = "macos")]
    caps.details.push("Native macOS provides admission and sampled memory monitoring, not hard aggregate memory protection.".into());
    caps
}

pub fn discover() -> ResourceSnapshot {
    let mut snapshot = ResourceSnapshot {
        cpu_millis: None,
        memory_available: None,
        provenance: Vec::new(),
        warnings: Vec::new(),
    };
    if let Ok(count) = std::thread::available_parallelism() {
        snapshot.cpu_millis = Some((count.get() as u64).saturating_mul(1000));
        snapshot
            .provenance
            .push(format!("available_parallelism={}", count.get()));
    }
    #[cfg(target_os = "linux")]
    discover_linux(&mut snapshot);
    #[cfg(target_os = "macos")]
    discover_macos(&mut snapshot);
    if snapshot.cpu_millis.is_none() {
        snapshot.warnings.push("CPU capacity is unknown".into());
    }
    if snapshot.memory_available.is_none() {
        snapshot
            .warnings
            .push("Memory headroom is unknown; automatic concurrency is limited to one job".into());
    }
    snapshot
}

fn bounded_text(path: impl AsRef<Path>) -> io::Result<String> {
    let mut value = String::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut value)?;
    if value.len() > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "OS data exceeds 1 MiB",
        ));
    }
    Ok(value)
}

fn read_number(path: impl AsRef<Path>) -> Option<u64> {
    bounded_text(path).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn minimum(current: &mut Option<u64>, value: u64) {
    *current = Some(current.map_or(value, |old| old.min(value)));
}

#[cfg(any(target_os = "linux", test))]
fn cpu_quota(value: &str) -> Option<u64> {
    let mut words = value.split_whitespace();
    let quota = words.next()?.parse::<u64>().ok()?;
    let period = words.next()?.parse::<u64>().ok()?;
    if period == 0 || quota == 0 {
        return None;
    }
    Some(quota.saturating_mul(1000).checked_div(period)?.max(1))
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct CgroupLocation {
    mount: PathBuf,
    current: PathBuf,
    hidden_ancestors: bool,
}

#[cfg(target_os = "linux")]
fn unescape_mount(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(target_os = "linux")]
fn cgroup_location() -> io::Result<CgroupLocation> {
    let membership = bounded_text("/proc/self/cgroup")?;
    let group = membership
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no unified cgroup membership"))?;
    if Path::new(group)
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unresolvable cgroup namespace membership",
        ));
    }
    let mounts = bounded_text("/proc/self/mountinfo")?;
    for line in mounts.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        if right.split_whitespace().next() != Some("cgroup2") {
            continue;
        }
        let fields: Vec<_> = left.split_whitespace().collect();
        if fields.len() < 5 {
            continue;
        }
        let root = PathBuf::from(unescape_mount(fields[3]));
        let mount = PathBuf::from(unescape_mount(fields[4]));
        let group_path = Path::new(group);
        let relative = group_path
            .strip_prefix(&root)
            .or_else(|_| group_path.strip_prefix("/"));
        let Ok(relative) = relative else { continue };
        let current = mount.join(relative);
        if current.join("cgroup.procs").exists() {
            return Ok(CgroupLocation {
                mount,
                current,
                hidden_ancestors: root != Path::new("/"),
            });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "unified cgroup mount/membership is not visible",
    ))
}

#[cfg(target_os = "linux")]
fn discover_linux(snapshot: &mut ResourceSnapshot) {
    // SAFETY: zeroed cpu_set_t is a valid output buffer of the exact supplied size.
    let mut affinity: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    // SAFETY: pointer and size refer to affinity, and pid 0 means this process.
    if unsafe { libc::sched_getaffinity(0, std::mem::size_of_val(&affinity), &mut affinity) } == 0 {
        // SAFETY: affinity was initialized by sched_getaffinity.
        let count = unsafe { libc::CPU_COUNT(&affinity) } as u64;
        if count > 0 {
            // available_parallelism may round a container quota to whole CPUs.
            // Start again from affinity, then apply every quota in millicores.
            snapshot.cpu_millis = Some(count * 1000);
            snapshot
                .provenance
                .push(format!("CPU affinity={count} CPUs"));
        }
    }
    if let Ok(info) = bounded_text("/proc/meminfo")
        && let Some(kib) = info.lines().find_map(|line| {
            line.strip_prefix("MemAvailable:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
        })
    {
        snapshot.memory_available = kib.checked_mul(1024);
        snapshot
            .provenance
            .push(format!("/proc/meminfo MemAvailable={kib} KiB"));
    }
    match cgroup_location() {
        Ok(location) => {
            if location.hidden_ancestors {
                snapshot
                    .warnings
                    .push("Cgroup mount hides ancestors; unseen constraints are unknown".into());
            }
            for path in location
                .current
                .ancestors()
                .take_while(|p| p.starts_with(&location.mount))
            {
                match bounded_text(path.join("cpu.max")) {
                    Ok(value) => {
                        if let Some(millis) = cpu_quota(&value) {
                            minimum(&mut snapshot.cpu_millis, millis);
                            snapshot.provenance.push(format!(
                                "{}={millis} millicores",
                                path.join("cpu.max").display()
                            ));
                        }
                    }
                    Err(error) if error.kind() != io::ErrorKind::NotFound => snapshot
                        .warnings
                        .push(format!("CPU constraint unreadable at {}", path.display())),
                    Err(_) => {}
                }
                match bounded_text(path.join("memory.max")) {
                    Ok(value) if value.trim() != "max" => match (
                        value.trim().parse::<u64>(),
                        read_number(path.join("memory.current")),
                    ) {
                        (Ok(limit), Some(used)) => {
                            let headroom = limit.saturating_sub(used);
                            minimum(&mut snapshot.memory_available, headroom);
                            snapshot
                                .provenance
                                .push(format!("{} headroom={headroom} bytes", path.display()));
                        }
                        _ => snapshot
                            .warnings
                            .push(format!("Memory headroom unreadable at {}", path.display())),
                    },
                    Err(error) if error.kind() != io::ErrorKind::NotFound => {
                        snapshot.warnings.push(format!(
                            "Memory constraint unreadable at {}",
                            path.display()
                        ))
                    }
                    _ => {}
                }
            }
        }
        Err(error) => snapshot
            .warnings
            .push(format!("Cgroup constraints unknown: {error}")),
    }
}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc exposes the stable Mach C ABI with migration notices.
fn discover_macos(snapshot: &mut ResourceSnapshot) {
    unsafe extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    // SAFETY: Mach returns a send right for this process; released below.
    let host = unsafe { libc::mach_host_self() };
    // SAFETY: vm_statistics64 is a C output structure accepting zero initialization.
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    // SAFETY: the buffer and count match HOST_VM_INFO64's ABI.
    let result = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    // SAFETY: deallocates only the send right acquired above, in this task.
    unsafe {
        mach_port_deallocate(libc::mach_task_self(), host);
    }
    // SAFETY: sysconf has no pointer arguments; negative results are handled.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if result == 0 && page_size > 0 {
        // Speculative pages already belong to free_count. Inactive pages are an
        // estimate of reclaimable capacity; never count compressor or swap as RAM.
        let pages = u64::from(stats.free_count) + u64::from(stats.inactive_count);
        snapshot.memory_available = pages.checked_mul(page_size as u64);
        snapshot.provenance.push("macOS Mach free+inactive pages (estimated reclaimable headroom; speculative pages counted once)".into());
    }
    let mut pressure: libc::c_int = 0;
    let mut size = std::mem::size_of_val(&pressure);
    // SAFETY: NUL-terminated static name; correctly sized output; no sysctl write.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.memorystatus_vm_pressure_level".as_ptr(),
            (&mut pressure as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 {
        snapshot
            .provenance
            .push(format!("macOS memory pressure level={pressure}"));
        if pressure > 1 {
            snapshot.memory_available = Some(0);
            snapshot
                .warnings
                .push("macOS reports elevated memory pressure; new admission is paused".into());
        }
    } else {
        snapshot.warnings.push(
            "macOS memory pressure signal is unavailable; reclaimable headroom is an estimate"
                .into(),
        );
    }
    snapshot
        .warnings
        .push("macOS memory observation is sampled and is not a hard whole-tree limit".into());
}

#[derive(Clone, Debug)]
pub struct TaskCgroup {
    path: PathBuf,
    initial_oom_kills: u64,
    initial_local_oom: u64,
}

impl TaskCgroup {
    pub fn attach(&self, pid: u32) -> io::Result<()> {
        fs::write(self.path.join("cgroup.procs"), pid.to_string())?;
        let members = bounded_text(self.path.join("cgroup.procs"))?;
        if !members.lines().any(|line| line.parse::<u32>() == Ok(pid)) {
            return Err(io::Error::other(
                "child cgroup attachment could not be verified",
            ));
        }
        Ok(())
    }

    pub fn memory_usage(&self) -> Option<u64> {
        read_number(self.path.join("memory.current"))
    }

    pub fn oom_kills(&self) -> u64 {
        if event_count(&self.path.join("memory.events.local"), "oom") <= self.initial_local_oom {
            return 0;
        }
        oom_kills_at(&self.path).saturating_sub(self.initial_oom_kills)
    }

    pub fn populated(&self) -> io::Result<bool> {
        let events = bounded_text(self.path.join("cgroup.events"))?;
        events
            .lines()
            .find_map(|line| line.strip_prefix("populated "))
            .map(|value| value.trim() != "0")
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing populated counter"))
    }

    pub fn kill(&self) -> io::Result<()> {
        fs::write(self.path.join("cgroup.kill"), "1")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn oom_kills_at(path: &Path) -> u64 {
    event_count(&path.join("memory.events"), "oom_kill")
}

fn event_count(path: &Path, field: &str) -> u64 {
    bounded_text(path)
        .ok()
        .and_then(|events| {
            events.lines().find_map(|line| {
                let (key, value) = line.split_once(' ')?;
                if key == field {
                    value.trim().parse::<u64>().ok()
                } else {
                    None
                }
            })
        })
        .unwrap_or(0)
}

pub struct Protection {
    pub capabilities: Capabilities,
    pub mode: EnforcementMode,
    run_root: Option<PathBuf>,
    workload: Option<PathBuf>,
    tasks: BTreeMap<String, TaskCgroup>,
}

impl Protection {
    pub fn prepare(
        mode: EnforcementMode,
        aggregate_cpu_millis: u64,
        aggregate_memory: Option<u64>,
        task_limits: &[(String, Option<u64>, Option<u64>)],
    ) -> Result<Self, String> {
        let mut protection = Self {
            capabilities: Capabilities::fallback(mode),
            mode,
            run_root: None,
            workload: None,
            tasks: BTreeMap::new(),
        };
        if mode == EnforcementMode::Off {
            return Ok(protection);
        }
        if mode == EnforcementMode::Required && aggregate_memory.is_none() {
            return Err("required protection needs a known aggregate memory bound".into());
        }
        #[cfg(target_os = "linux")]
        let setup = protection.setup_linux(aggregate_cpu_millis, aggregate_memory, task_limits);
        #[cfg(not(target_os = "linux"))]
        let setup: io::Result<()> = {
            let _ = (aggregate_cpu_millis, aggregate_memory, task_limits);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "hard CPU/aggregate memory protection requires delegated Linux cgroup v2",
            ))
        };
        match setup {
            Ok(()) => Ok(protection),
            Err(error) => {
                if let Err(cleanup_error) = protection.cleanup() {
                    return Err(format!(
                        "protection setup failed ({error}); owned cgroup cleanup failed ({cleanup_error})"
                    ));
                }
                if mode == EnforcementMode::Required {
                    Err(format!(
                        "required protection unavailable before launch: {error}"
                    ))
                } else {
                    protection.capabilities = Capabilities::fallback(mode);
                    protection
                        .capabilities
                        .details
                        .push(format!("Hard protection unavailable: {error}"));
                    Ok(protection)
                }
            }
        }
    }

    pub fn task(&self, id: &str) -> Option<TaskCgroup> {
        self.tasks.get(id).cloned()
    }

    pub fn aggregate_usage(&self) -> Option<u64> {
        read_number(self.workload.as_ref()?.join("memory.current"))
    }

    pub fn aggregate_oom_kills(&self) -> u64 {
        self.workload.as_ref().map_or(0, |path| {
            if event_count(&path.join("memory.events.local"), "oom") > 0 {
                oom_kills_at(path)
            } else {
                0
            }
        })
    }

    pub fn watchdog_enabled(&self) -> bool {
        self.mode == EnforcementMode::Auto
    }

    /// Remove only empty directories inside this run's exclusively created tree.
    /// Call explicitly to surface cleanup failures; Drop is a final best effort.
    pub fn cleanup(&mut self) -> io::Result<()> {
        if let Some(path) = &self.run_root {
            let mut count = 0;
            remove_owned_empty_tree(path, 0, &mut count)?;
        }
        self.tasks.clear();
        self.workload = None;
        self.run_root = None;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn setup_linux(
        &mut self,
        cpu: u64,
        memory: Option<u64>,
        limits: &[(String, Option<u64>, Option<u64>)],
    ) -> io::Result<()> {
        if cpu == 0 || memory == Some(0) {
            return Err(io::Error::other("protection bounds must be positive"));
        }
        let location = cgroup_location()?;
        let mut parent = location.current.clone();
        let available = bounded_text(parent.join("cgroup.subtree_control"))?;
        // A delegated runner commonly lives in an unlimited supervisor leaf.
        // We may create a sibling only if doing so bypasses no leaf constraint.
        if !has_controllers(&available)
            && let Some(candidate) = parent.parent().filter(|p| p.starts_with(&location.mount))
            && unlimited_leaf(&parent, candidate)
            && bounded_text(candidate.join("cgroup.subtree_control"))
                .is_ok_and(|value| has_controllers(&value))
        {
            parent = candidate.to_path_buf();
        }
        let available = bounded_text(parent.join("cgroup.subtree_control"))?;
        let cpu_supported = available.split_whitespace().any(|word| word == "cpu");
        let memory_supported = available.split_whitespace().any(|word| word == "memory");
        if (self.mode == EnforcementMode::Required && !(cpu_supported && memory_supported))
            || !(cpu_supported || memory_supported)
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "cpu and memory controllers must already be enabled by the delegated runner; parent controller settings are not modified",
            ));
        }
        let controllers = match (cpu_supported, memory_supported) {
            (true, true) => "+cpu +memory",
            (true, false) => "+cpu",
            (false, true) => "+memory",
            (false, false) => unreachable!(),
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let run = parent.join(format!("takt-{}-{unique}", std::process::id()));
        fs::create_dir(&run)?;
        self.run_root = Some(run.clone());
        fs::write(run.join("cgroup.subtree_control"), controllers)?;
        let workload = run.join("workload");
        fs::create_dir(&workload)?;
        self.workload = Some(workload.clone());
        let mut details = Vec::new();
        if cpu_supported {
            write_verified(&workload.join("cpu.max"), &cpu_max(cpu)?)?;
        }
        if memory_supported {
            if let Some(memory) = memory
                && let Some(detail) = write_memory_cap(&workload.join("memory.max"), memory)?
            {
                details.push(detail);
            }
            write_verified(&workload.join("memory.oom.group"), "1")?;
        }
        let swap =
            memory_supported && write_verified(&workload.join("memory.swap.max"), "0").is_ok();
        fs::write(workload.join("cgroup.subtree_control"), controllers)?;
        for (index, (id, task_cpu, task_memory)) in limits.iter().enumerate() {
            let path = workload.join(format!("task-{index}"));
            fs::create_dir(&path)?;
            self.tasks.insert(
                id.clone(),
                TaskCgroup {
                    initial_oom_kills: 0,
                    initial_local_oom: 0,
                    path: path.clone(),
                },
            );
            if cpu_supported && let Some(cpu) = task_cpu {
                write_verified(&path.join("cpu.max"), &cpu_max(*cpu)?)?;
            }
            if memory_supported {
                if let Some(memory) = task_memory
                    && let Some(detail) = write_memory_cap(&path.join("memory.max"), *memory)?
                {
                    details.push(detail);
                }
                write_verified(&path.join("memory.oom.group"), "1")?;
            }
            // Test required writable interfaces before starting any project command.
            fs::OpenOptions::new()
                .write(true)
                .open(path.join("cgroup.procs"))?;
            fs::OpenOptions::new()
                .write(true)
                .open(path.join("cgroup.kill"))?;
            if !path.join("cgroup.events").is_file() {
                return Err(io::Error::other(
                    "cgroup population observation unavailable",
                ));
            }
        }
        details.push(format!("Owned subtree: {}. Supervisor remains outside workload memory cap in its original cgroup.", run.display()));
        details.push("Same-UID workloads are trusted: intentional migration to a writable external cgroup can escape containment.".into());
        self.capabilities = Capabilities {
            cpu: if cpu_supported {
                "hard_cgroup_bandwidth"
            } else {
                "admission_only"
            }
            .into(),
            aggregate_memory: if memory_supported && memory.is_some() {
                "hard_cgroup_max"
            } else {
                "best_effort_watchdog"
            }
            .into(),
            per_task_memory: if memory_supported {
                "hard_cgroup_max_for_explicit_limits"
            } else {
                "best_effort_watchdog"
            }
            .into(),
            process_tree_containment: "cgroup_v2".into(),
            descendant_accounting: "cgroup_v2".into(),
            swap: if swap {
                "no_additional_workload_swap"
            } else {
                "external_policy_only"
            }
            .into(),
            details,
        };
        Ok(())
    }
}

impl Drop for Protection {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

fn remove_owned_empty_tree(path: &Path, depth: usize, count: &mut usize) -> io::Result<()> {
    *count += 1;
    if depth > 64 || *count > 4096 {
        return Err(io::Error::other(
            "owned cgroup cleanup exceeds traversal bound",
        ));
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            remove_owned_empty_tree(&entry.path(), depth + 1, count)?;
        }
    }
    fs::remove_dir(path)
}

#[cfg(target_os = "linux")]
fn has_controllers(value: &str) -> bool {
    value.split_whitespace().any(|word| word == "cpu")
        && value.split_whitespace().any(|word| word == "memory")
}

#[cfg(target_os = "linux")]
fn unlimited_leaf(leaf: &Path, parent: &Path) -> bool {
    for name in [
        "memory.max",
        "memory.high",
        "memory.swap.max",
        "memory.zswap.max",
        "pids.max",
    ] {
        match bounded_text(leaf.join(name)) {
            Ok(value) if value.trim() == "max" => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return false,
        }
    }
    match bounded_text(leaf.join("cpu.max")) {
        Ok(value) if value.starts_with("max ") => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => return false,
    }
    match bounded_text(leaf.join("io.max")) {
        Ok(value) if value.trim().is_empty() => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        _ => return false,
    }
    for name in ["cpuset.cpus.effective", "cpuset.mems.effective"] {
        match bounded_text(leaf.join(name)) {
            Ok(value) => {
                if !bounded_text(parent.join(name))
                    .is_ok_and(|inherited| value.trim() == inherited.trim())
                {
                    return false;
                }
            }
            // Without cpuset enabled below the parent, the leaf inherits the
            // parent's placement and has no cpuset files of its own. A sibling
            // then retains that same ceiling. Other read errors are unknown.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if !bounded_text(parent.join("cgroup.subtree_control"))
                    .is_ok_and(|value| !value.split_whitespace().any(|word| word == "cpuset"))
                {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

#[cfg(target_os = "linux")]
fn write_verified(path: &Path, value: &str) -> io::Result<()> {
    fs::write(path, value)?;
    let observed = bounded_text(path)?;
    if observed.split_whitespace().ne(value.split_whitespace()) {
        return Err(io::Error::other(format!(
            "controller verification failed: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn write_memory_cap(path: &Path, requested: u64) -> io::Result<Option<String>> {
    // SAFETY: sysconf has no pointer arguments and negative errors are checked.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page <= 0 {
        return Err(io::Error::other("system page size unavailable"));
    }
    // Kernel memory controls operate in pages. Round down explicitly so the
    // applied bound never exceeds the user's bytes and verify exactly that value.
    let effective = requested / page as u64 * page as u64;
    if effective == 0 {
        return Err(io::Error::other(
            "hard memory limit is smaller than one system page",
        ));
    }
    write_verified(path, &effective.to_string())?;
    Ok((requested != effective).then(|| {
        format!(
            "{}: requested {requested} bytes, applied {effective} bytes at system page granularity",
            path.display()
        )
    }))
}

#[cfg(target_os = "linux")]
fn cpu_max(millis: u64) -> io::Result<String> {
    if millis == 0 {
        return Err(io::Error::other("CPU quota must be positive"));
    }
    // CFS requires at least 1 ms quota. Use its maximum 1 s period only for the
    // smallest representable fractional capacities, preserving 1-9 millicores.
    let period = if millis < 10 {
        1_000_000u64
    } else {
        100_000u64
    };
    let quota = millis
        .checked_mul(period / 1000)
        .ok_or_else(|| io::Error::other("CPU quota overflow"))?;
    Ok(format!("{quota} {period}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_cpu_quota_remains_fractional() {
        assert_eq!(cpu_quota("25000 100000\n"), Some(250));
        assert_eq!(cpu_quota("max 100000"), None);
        assert_eq!(cpu_quota("1 0"), None);
    }

    #[test]
    fn discovery_is_positive_or_explicitly_unknown() {
        let snapshot = discover();
        assert!(snapshot.cpu_millis.is_none_or(|cpu| cpu > 0));
        if snapshot.memory_available.is_none() {
            assert!(!snapshot.warnings.is_empty());
        }
    }

    #[test]
    fn off_never_creates_controllers() {
        let protection = Protection::prepare(EnforcementMode::Off, 1000, Some(1024), &[]).unwrap();
        assert!(protection.run_root.is_none());
        assert_eq!(protection.capabilities.aggregate_memory, "observation_only");
        assert!(!protection.watchdog_enabled());
    }

    #[test]
    fn required_unknown_memory_is_rejected_before_setup() {
        assert!(Protection::prepare(EnforcementMode::Required, 1000, None, &[]).is_err());
    }

    #[test]
    fn oom_kill_without_local_limit_event_is_not_own_limit_evidence() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("memory.events"),
            "oom 0\noom_kill 1\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("memory.events.local"),
            "oom 0\noom_kill 1\n",
        )
        .unwrap();
        let group = TaskCgroup {
            path: directory.path().into(),
            initial_oom_kills: 0,
            initial_local_oom: 0,
        };
        assert_eq!(group.oom_kills(), 0);
        fs::write(
            directory.path().join("memory.events.local"),
            "oom 1\noom_kill 1\n",
        )
        .unwrap();
        assert_eq!(group.oom_kills(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn tiny_cpu_quota_respects_kernel_minimum() {
        assert_eq!(cpu_max(1).unwrap(), "1000 1000000");
        assert_eq!(cpu_max(250).unwrap(), "25000 100000");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn supervisor_without_enabled_cpuset_inherits_parent_ceiling() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path();
        let leaf = parent.join("supervisor");
        fs::create_dir(&leaf).unwrap();
        fs::write(parent.join("cgroup.subtree_control"), "cpu memory pids\n").unwrap();
        fs::write(parent.join("cpuset.cpus.effective"), "0-3\n").unwrap();
        fs::write(parent.join("cpuset.mems.effective"), "0\n").unwrap();
        for name in ["memory.max", "memory.high", "memory.swap.max", "pids.max"] {
            fs::write(leaf.join(name), "max\n").unwrap();
        }
        fs::write(leaf.join("cpu.max"), "max 100000\n").unwrap();
        assert!(unlimited_leaf(&leaf, parent));

        // A missing file cannot stand in for a controller that is enabled.
        fs::write(
            parent.join("cgroup.subtree_control"),
            "cpu memory cpuset pids\n",
        )
        .unwrap();
        assert!(!unlimited_leaf(&leaf, parent));
        fs::write(leaf.join("cpuset.cpus.effective"), "0-3\n").unwrap();
        fs::write(leaf.join("cpuset.mems.effective"), "0\n").unwrap();
        assert!(unlimited_leaf(&leaf, parent));

        fs::write(leaf.join("cpuset.cpus.effective"), "0-1\n").unwrap();
        assert!(!unlimited_leaf(&leaf, parent));
        fs::write(leaf.join("cpuset.cpus.effective"), "0-3\n").unwrap();
        fs::write(leaf.join("memory.max"), "67108864\n").unwrap();
        assert!(!unlimited_leaf(&leaf, parent));
    }
}

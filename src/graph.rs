//! Graph validation, selection closure, and deterministic dependency ordering.
use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub roots: BTreeSet<String>,
    pub tasks: BTreeSet<String>,
}
pub fn select(config: &Config, names: &[String]) -> Result<Selection, String> {
    if names.is_empty() {
        return Err("select at least one task or group".into());
    }
    let mut roots = BTreeSet::new();
    for name in names {
        if config.tasks.contains_key(name) {
            roots.insert(name.clone());
        } else if let Some(group) = config.groups.get(name) {
            roots.extend(group.iter().cloned());
        } else {
            return Err(format!("unknown task or group `{name}`"));
        }
    }
    let mut tasks = BTreeSet::new();
    let mut stack: Vec<_> = roots.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        if tasks.insert(id.clone()) {
            let task = config
                .tasks
                .get(&id)
                .ok_or_else(|| format!("unknown task `{id}`"))?;
            stack.extend(task.depends_on.iter().map(|d| d.task().to_owned()));
        }
    }
    Ok(Selection { roots, tasks })
}
pub fn validate(config: &Config) -> Result<(), String> {
    if config.tasks.is_empty() {
        return Err("configuration must define at least one task".into());
    }
    for (name, task) in &config.tasks {
        validate_name(name)?;
        match (&task.command, &task.shell) {
            (Some(argv), None)
                if !argv.is_empty()
                    && !argv[0].is_empty()
                    && !argv.iter().any(|a| a.contains('\0')) => {}
            (None, Some(shell)) if !shell.is_empty() && !shell.contains('\0') => {}
            _ => {
                return Err(format!(
                    "task `{name}` needs exactly one nonempty command array or shell string without NUL bytes"
                ));
            }
        }
        if task.kind == TaskKind::Task && task.readiness.is_some() {
            return Err(format!("finite task `{name}` cannot define readiness"));
        }
        if task.kind == TaskKind::Service && task.success_codes.is_some() {
            return Err(format!("service `{name}` cannot define success_codes"));
        }
        if let Some(codes) = &task.success_codes {
            validate_unique(codes, &format!("task `{name}` success_codes"))?;
        }
        if let Some(probe) = &task.readiness {
            validate_probe(probe).map_err(|e| format!("task `{name}` readiness: {e}"))?;
        }
        let mut edges = BTreeSet::new();
        for dependency in &task.depends_on {
            let parent = dependency.task();
            if !edges.insert(parent) {
                return Err(format!("task `{name}` has duplicate dependency `{parent}`"));
            }
            if name == parent {
                return Err(format!("task `{name}` depends on itself"));
            }
            let target = config
                .tasks
                .get(parent)
                .ok_or_else(|| format!("task `{name}` depends on missing task `{parent}`"))?;
            match dependency {
                Dependency::Exit { codes, .. } => {
                    if target.kind != TaskKind::Task {
                        return Err(format!(
                            "task `{name}` has an exit edge to service `{parent}`"
                        ));
                    }
                    validate_unique(codes, &format!("task `{name}` dependency `{parent}` codes"))?;
                }
                Dependency::Ready { .. } => {
                    if target.kind != TaskKind::Service {
                        return Err(format!(
                            "task `{name}` has a ready edge to finite task `{parent}`"
                        ));
                    }
                    if target.readiness.is_none() {
                        return Err(format!(
                            "service `{parent}` needs a readiness probe for dependent `{name}`"
                        ));
                    }
                }
            }
        }
    }
    for (name, members) in &config.groups {
        validate_name(name)?;
        if config.tasks.contains_key(name) {
            return Err(format!("task and group names collide at `{name}`"));
        }
        validate_unique(members, &format!("group `{name}`"))?;
        for member in members {
            if !config.tasks.contains_key(member) {
                return Err(format!("group `{name}` references missing task `{member}`"));
            }
        }
    }
    topological(config, config.tasks.keys().cloned().collect())?;
    Ok(())
}
fn validate_name(name: &str) -> Result<(), String> {
    let mut bytes = name.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        || !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(format!(
            "invalid task/group name `{name}`; use a letter followed by letters, digits, `_`, or `-`"
        ));
    }
    Ok(())
}
fn validate_unique<T: Ord>(values: &[T], context: &str) -> Result<(), String> {
    if values.is_empty() {
        return Err(format!("{context} cannot be empty"));
    }
    if values.iter().collect::<BTreeSet<_>>().len() != values.len() {
        return Err(format!("{context} contains duplicate entries"));
    }
    Ok(())
}
pub fn validate_probe(probe: &Probe) -> Result<(), String> {
    match probe {
        Probe::Tcp { host, port, .. } => {
            validate_host(host)?;
            if *port == 0 {
                return Err("port must be in 1..65535".into());
            }
        }
        Probe::Udp {
            host,
            port,
            payload,
            expect,
            ..
        } => {
            validate_host(host)?;
            if *port == 0 {
                return Err("port must be in 1..65535".into());
            }
            if payload.decode()?.len() > 65_507 {
                return Err("UDP payload exceeds 65,507 bytes".into());
            }
            if expect.bytes().decode()?.len() > 65_507 {
                return Err("UDP expectation exceeds 65,507 bytes".into());
            }
        }
        Probe::Http {
            url,
            status_codes,
            body_contains,
            ..
        } => {
            validate_http_url(url)?;
            if let Some(codes) = status_codes {
                validate_unique(codes, "HTTP status_codes")?;
                if codes.iter().any(|c| !(100..=599).contains(c)) {
                    return Err("HTTP status codes must be in 100..599".into());
                }
            }
            if body_contains
                .as_ref()
                .is_some_and(|b| b.is_empty() || b.len() > 65_536)
            {
                return Err("HTTP body_contains must contain 1..65,536 bytes".into());
            }
        }
        Probe::Stdio { contains, .. } => {
            if contains.is_empty() || contains.len() > 65_536 {
                return Err("stdio contains must contain 1..65,536 bytes".into());
            }
        }
    }
    Ok(())
}
fn validate_host(host: &str) -> Result<(), String> {
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    if host.len() > 253
        || host.trim_end_matches('.').split('.').any(|part| {
            part.is_empty()
                || part.len() > 63
                || part.starts_with('-')
                || part.ends_with('-')
                || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err("invalid IP address or DNS hostname".into());
    }
    Ok(())
}
pub fn validate_http_url(url: &str) -> Result<(), String> {
    // Use the runtime HTTP library's URI grammar as well as our scheme/host policy.
    let _: ureq_proto::http::Uri = url.parse().map_err(|_| "invalid HTTP URL syntax")?;
    let rest = url
        .strip_prefix("http://")
        .ok_or("HTTP readiness supports only http:// URLs; HTTPS is deferred")?;
    if rest
        .bytes()
        .any(|b| b.is_ascii_whitespace() || b.is_ascii_control())
        || rest.contains(['#', '@'])
    {
        return Err("invalid HTTP URL (fragments and userinfo are unsupported)".into());
    }
    let authority = rest.split(['/', '?']).next().unwrap_or("");
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, tail) = bracketed.split_once(']').ok_or("invalid IPv6 HTTP URL")?;
        host.parse::<std::net::Ipv6Addr>()
            .map_err(|_| "invalid IPv6 HTTP URL")?;
        if !tail.is_empty() {
            validate_port(tail.strip_prefix(':').ok_or("invalid HTTP authority")?)?;
        }
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority, None), |(h, p)| (h, Some(p)));
        validate_host(host)?;
        if let Some(port) = port {
            validate_port(port)?;
        }
    }
    Ok(())
}
fn validate_port(port: &str) -> Result<(), String> {
    if !port.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u16>().ok().is_some_and(|n| n > 0)
    {
        Ok(())
    } else {
        Err("invalid HTTP port".into())
    }
}

pub fn topological(config: &Config, selected: BTreeSet<String>) -> Result<Vec<String>, String> {
    let mut remaining: BTreeMap<String, usize> = selected
        .iter()
        .map(|id| {
            (
                id.clone(),
                config.tasks[id]
                    .depends_on
                    .iter()
                    .filter(|d| selected.contains(d.task()))
                    .count(),
            )
        })
        .collect();
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in &selected {
        for dep in &config.tasks[id].depends_on {
            if selected.contains(dep.task()) {
                children.entry(dep.task()).or_default().push(id);
            }
        }
    }
    let mut ready: BTreeSet<String> = remaining
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut order = Vec::with_capacity(selected.len());
    while let Some(id) = ready.pop_first() {
        for child in children.get(id.as_str()).into_iter().flatten() {
            if let Some(n) = remaining.get_mut(*child) {
                *n -= 1;
                if *n == 0 {
                    ready.insert((*child).to_owned());
                }
            }
        }
        remaining.remove(&id);
        order.push(id);
    }
    if remaining.is_empty() {
        return Ok(order);
    }
    let mut path = Vec::new();
    let mut positions = BTreeMap::new();
    let mut id = remaining
        .keys()
        .next()
        .cloned()
        .ok_or("cycle detection failed")?;
    loop {
        if let Some(&start) = positions.get(&id) {
            path.push(id);
            return Err(format!("dependency cycle: {}", path[start..].join(" -> ")));
        }
        positions.insert(id.clone(), path.len());
        path.push(id.clone());
        id = config.tasks[&id]
            .depends_on
            .iter()
            .map(Dependency::task)
            .find(|d| remaining.contains_key(*d))
            .ok_or("cycle detection failed")?
            .to_owned();
    }
}
pub const MAX_SERVICE_ANCESTOR_ENTRIES: usize = 100_000;
pub const MAX_SERVICE_ANCESTOR_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_ANCESTRY_STEPS: usize = 1_000_000;

/// Services reachable from a task remain needed until that task settles.
/// Compute each parent once; account for retained data before copying any name.
pub fn service_ancestry(
    config: &Config,
    selected: &BTreeSet<String>,
) -> Result<BTreeMap<String, BTreeSet<String>>, String> {
    let mut ancestry: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let (mut entries, mut bytes, mut steps) = (0, 0, 0);
    for id in topological(config, selected.clone())? {
        let mut services = BTreeSet::new();
        for edge in &config.tasks[&id].depends_on {
            ancestry_step(&mut steps)?;
            let parent = edge.task();
            let parent_ancestors = ancestry
                .get(parent)
                .ok_or_else(|| format!("selected graph is missing dependency `{parent}`"))?;
            let parent_service = (config.tasks[parent].kind == TaskKind::Service).then_some(parent);
            for ancestor in parent_ancestors
                .iter()
                .map(String::as_str)
                .chain(parent_service)
            {
                ancestry_step(&mut steps)?;
                if services.contains(ancestor) {
                    continue;
                }
                if entries == MAX_SERVICE_ANCESTOR_ENTRIES {
                    return Err(ancestry_budget_error(
                        "100,000 retained service-ancestor entries",
                    ));
                }
                if ancestor.len() > MAX_SERVICE_ANCESTOR_BYTES - bytes {
                    return Err(ancestry_budget_error(
                        "8 MiB of retained service-ancestor names",
                    ));
                }
                entries += 1;
                bytes += ancestor.len();
                services.insert(ancestor.to_owned());
            }
        }
        ancestry.insert(id, services);
    }
    Ok(ancestry)
}

fn ancestry_step(steps: &mut usize) -> Result<(), String> {
    if *steps == MAX_ANCESTRY_STEPS {
        return Err(ancestry_budget_error(
            "1,000,000 service-ancestry planning steps",
        ));
    }
    *steps += 1;
    Ok(())
}

fn ancestry_budget_error(limit: &str) -> String {
    format!(
        "selected dependency graph exceeds planning budget of {limit}; select fewer tasks or split this run"
    )
}

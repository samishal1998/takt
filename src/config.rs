//! Strict, bounded configuration loading. No interpolation, tags, or includes.
use crate::{graph, model::*};
use granit_parser::{Event, Parser, ScalarStyle};
use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeOwned, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const MAX_DEPTH: usize = 64;
pub const MAX_EVENTS: usize = 100_000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Yaml,
    Json,
}

pub fn discover(directory: &Path, explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(if path.is_absolute() {
            path.to_owned()
        } else {
            directory.join(path)
        });
    }
    let found: Vec<_> = ["takt.yaml", "takt.yml", "takt.json"]
        .iter()
        .map(|p| directory.join(p))
        .filter(|p| p.exists())
        .collect();
    match found.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err("no takt.yaml, takt.yml, or takt.json found in the working directory".into()),
        _ => Err("multiple Takt configuration files found; choose one with --file".into()),
    }
}
pub fn load(path: &Path) -> Result<Config, String> {
    let (source, format, base_dir) = read_source(path)?;
    parse(&source, format, &base_dir).map_err(|e| format!("{}: {e}", path.display()))
}

/// Sources contain only explicitly present file options, including explicit auto/off.
pub fn load_with_sources(path: &Path) -> Result<(Config, BTreeMap<String, String>), String> {
    let (source, format, base_dir) = read_source(path)?;
    parse_with_sources(&source, format, &base_dir).map_err(|e| format!("{}: {e}", path.display()))
}

fn read_source(path: &Path) -> Result<(String, Format, PathBuf), String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .take((MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err("configuration exceeds the 1 MiB input limit".into());
    }
    let source = String::from_utf8(bytes).map_err(|_| "configuration is not UTF-8")?;
    let format = if path.extension().is_some_and(|e| e == "json") {
        Format::Json
    } else {
        Format::Yaml
    };
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path)
    };
    Ok((
        source,
        format,
        absolute.parent().unwrap_or(Path::new(".")).to_owned(),
    ))
}
pub fn parse(source: &str, format: Format, base_dir: &Path) -> Result<Config, String> {
    let mut config: Config = decode(source, format)?;
    normalize(&mut config, base_dir)?;
    graph::validate(&config)?;
    Ok(config)
}

pub fn parse_with_sources(
    source: &str,
    format: Format,
    base_dir: &Path,
) -> Result<(Config, BTreeMap<String, String>), String> {
    let config = parse(source, format, base_dir)?;
    // IgnoredAny consumes task content without creating an unconstrained Value tree.
    #[derive(Deserialize)]
    struct Presence {
        enforcement: Option<IgnoredAny>,
        failure: Option<IgnoredAny>,
        service_failure: Option<IgnoredAny>,
        output: Option<IgnoredAny>,
    }
    let presence: Presence = decode(source, format)?;
    let mut sources = BTreeMap::new();
    for (key, present) in [
        ("jobs", config.scheduling.jobs.is_some()),
        ("cpu", config.scheduling.cpu.is_some()),
        ("memory", config.scheduling.memory.is_some()),
        ("reserve-memory", config.scheduling.reserve_memory.is_some()),
        ("queue-timeout", config.scheduling.queue_timeout.is_some()),
        ("enforcement", presence.enforcement.is_some()),
        ("failure", presence.failure.is_some()),
        ("service-failure", presence.service_failure.is_some()),
        ("output", presence.output.is_some()),
    ] {
        if present {
            sources.insert(key.into(), "configuration".into());
        }
    }
    Ok((config, sources))
}

fn decode<T: DeserializeOwned>(source: &str, format: Format) -> Result<T, String> {
    if source.len() > MAX_INPUT_BYTES {
        return Err("configuration exceeds the 1 MiB input limit".into());
    }
    match format {
        Format::Json => {
            check_json_depth(source)?;
            // Validate duplicate keys without retaining a second complete value tree.
            let _: UniqueValue = serde_json::from_str(source).map_err(|e| e.to_string())?;
            serde_json::from_str(source).map_err(|e| e.to_string())
        }
        Format::Yaml => {
            check_yaml(source)?;
            let options = serde_saphyr::options! {
                duplicate_keys:serde_saphyr::DuplicateKeyPolicy::Error,
                merge_keys:serde_saphyr::MergeKeyPolicy::Error,
                strict_booleans:true,
                no_schema:true,
                reject_unsupported_tags:true,
                with_snippet:false,
                alias_limits:serde_saphyr::alias_limits! {
                    max_total_replayed_events:MAX_EVENTS,
                    max_replay_stack_depth:MAX_DEPTH,
                    max_alias_expansions_per_anchor:1024,
                },
                budget:serde_saphyr::budget! {
                    max_reader_input_bytes:Some(MAX_INPUT_BYTES),
                    max_events:MAX_EVENTS,
                    max_depth:MAX_DEPTH,
                    max_nodes:MAX_EVENTS,
                    max_aliases:1024,
                    max_anchors:1024,
                    max_documents:1,
                    max_recorded_anchor_events:MAX_EVENTS,
                    max_recorded_anchor_bytes:2*1024*1024,
                    max_total_scalar_bytes:2*1024*1024,
                    max_merge_keys:0,
                },
            };
            serde_saphyr::from_str_with_options(source, options).map_err(|e| e.to_string())
        }
    }
}
fn check_yaml(source: &str) -> Result<(), String> {
    let mut options = granit_parser::Options::default();
    options.flow_nesting_limit = MAX_DEPTH;
    options.block_nesting_limit = MAX_DEPTH;
    options.set_emit_comments(false);
    let parser = Parser::new_from_str_with_options(source, options);
    let mut depth = 0_usize;
    let mut anchors = 0;
    let mut aliases = 0;
    for (count, event) in parser.enumerate() {
        if count >= MAX_EVENTS {
            return Err("YAML exceeds the 100,000 event limit".into());
        }
        let (event, span) = event.map_err(|e| e.to_string())?;
        if event.tag().is_some() {
            return Err(format!("explicit YAML tags are unsupported ({span:?})"));
        }
        if event.scalar().is_some_and(|(value, style)| {
            style == ScalarStyle::Plain
                && (value.is_empty() || value == "~" || value.eq_ignore_ascii_case("null"))
        }) {
            return Err("configuration values cannot be null; omit optional fields instead".into());
        }
        if let Some((value, ScalarStyle::Plain)) = event.scalar()
            && value.parse::<f64>().is_ok()
            && value.contains(['.', 'e', 'E'])
        {
            check_numeric_precision(value)?;
        }
        if event.anchor_id().is_some_and(|id| id != 0) {
            anchors += 1;
        }
        if event.alias_id().is_some() {
            aliases += 1;
        }
        if anchors > 1024 || aliases > 1024 {
            return Err("YAML exceeds the 1,024 anchor/alias limit".into());
        }
        match event {
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err("configuration exceeds the depth limit of 64".into());
                }
            }
            Event::SequenceEnd | Event::MappingEnd => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}
fn check_json_depth(source: &str) -> Result<(), String> {
    let (mut quoted, mut escaped, mut depth, mut events) = (false, false, 0_usize, 0_usize);
    let bytes = source.as_bytes();
    let mut position = 0;
    while position < bytes.len() {
        let b = bytes[position];
        position += 1;
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
            continue;
        }
        match b {
            b'"' => {
                quoted = true;
                events += 1;
            }
            b'{' | b'[' => {
                depth += 1;
                events += 1;
                if depth > MAX_DEPTH {
                    return Err("configuration exceeds the depth limit of 64".into());
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            b',' => events += 1,
            b'-' | b'0'..=b'9' => {
                let start = position - 1;
                while position < bytes.len()
                    && (bytes[position].is_ascii_digit()
                        || matches!(bytes[position], b'.' | b'e' | b'E' | b'+' | b'-'))
                {
                    position += 1;
                }
                let number = &source[start..position];
                if number.contains(['.', 'e', 'E']) {
                    check_numeric_precision(number)?;
                }
            }
            _ => {}
        }
        if events > MAX_EVENTS {
            return Err("JSON exceeds the 100,000 event limit".into());
        }
    }
    Ok(())
}

/// Validate decimal precision before a JSON/YAML deserializer rounds to f64.
/// CPU is the only fractional numeric field in the schema; all other numbers are integers.
fn check_numeric_precision(number: &str) -> Result<(), String> {
    let error = "numeric value cannot be represented exactly in positive integer millicores";
    let (mantissa, exponent) = number.split_once(['e', 'E']).map_or((number, 0), |(m, e)| {
        (m, e.parse::<i32>().unwrap_or(i32::MAX))
    });
    if mantissa.starts_with('-') || exponent == i32::MAX {
        return Err(error.into());
    }
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits = String::with_capacity(mantissa.len());
    digits.push_str(whole);
    digits.push_str(fraction);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error.into());
    }
    let scale = 3_i64 + exponent as i64 - fraction.len() as i64;
    if scale < 0 {
        let excess = usize::try_from(-scale).map_err(|_| error)?;
        if excess >= digits.len() || !digits[digits.len() - excess..].bytes().all(|b| b == b'0') {
            return Err(error.into());
        }
        digits.truncate(digits.len() - excess);
    }
    let value = digits.parse::<u64>().map_err(|_| error)?;
    let value = if scale > 0 {
        u32::try_from(scale)
            .ok()
            .and_then(|scale| 10_u64.checked_pow(scale))
            .and_then(|factor| value.checked_mul(factor))
            .ok_or(error)?
    } else {
        value
    };
    if value == 0 {
        return Err(error.into());
    }
    Ok(())
}
struct UniqueValue;
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut keys = BTreeSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(de::Error::custom(format!("duplicate key `{key}`")));
                    }
                    let _: UniqueValue = map.next_value()?;
                }
                Ok(UniqueValue)
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                while seq.next_element::<UniqueValue>()?.is_some() {}
                Ok(UniqueValue)
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue)
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                if v.is_finite() {
                    Ok(UniqueValue)
                } else {
                    Err(de::Error::custom("non-finite number"))
                }
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue)
            }
            fn visit_string<E: de::Error>(self, _: String) -> Result<Self::Value, E> {
                Ok(UniqueValue)
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Err(de::Error::custom(
                    "configuration values cannot be null; omit optional fields instead",
                ))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Err(de::Error::custom(
                    "configuration values cannot be null; omit optional fields instead",
                ))
            }
        }
        d.deserialize_any(UniqueVisitor)
    }
}

pub fn normalize(config: &mut Config, base_dir: &Path) -> Result<(), String> {
    if config.version != 1 {
        return Err(format!(
            "unsupported configuration version {}; expected 1",
            config.version
        ));
    }
    if matches!(config.scheduling.jobs, Some(Auto::Value(0))) {
        return Err("scheduling.jobs must be positive".into());
    }
    validate_env("defaults", &config.defaults.env, &config.defaults.unset_env)?;
    for (name, task) in &mut config.tasks {
        validate_env(name, &task.env, &task.unset_env)?;
        task.resources.inherit(config.defaults.resources);
        task.limits.inherit(config.defaults.limits);
        task.timeout = task.timeout.or(config.defaults.timeout);
        task.stop_grace = task.stop_grace.or(config.defaults.stop_grace);
        let mut env = config.defaults.env.clone();
        let mut unset: BTreeSet<_> = config.defaults.unset_env.iter().cloned().collect();
        for (key, value) in &task.env {
            unset.remove(key);
            env.insert(key.clone(), value.clone());
        }
        for key in &task.unset_env {
            env.remove(key);
            unset.insert(key.clone());
        }
        task.env = env;
        task.unset_env = unset.into_iter().collect();
        let cwd = task.cwd.as_deref().unwrap_or(Path::new("."));
        if cwd.as_os_str().is_empty() {
            return Err(format!("task `{name}` has an empty cwd"));
        }
        task.cwd = Some(if cwd.is_absolute() {
            cwd.to_owned()
        } else {
            base_dir.join(cwd)
        });
    }
    Ok(())
}
fn validate_env(
    context: &str,
    env: &BTreeMap<String, String>,
    unset: &[String],
) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for key in env.keys().chain(unset.iter()) {
        if key.is_empty() || key.contains(['=', '\0']) {
            return Err(format!("{context}: invalid environment variable name"));
        }
    }
    if env.values().any(|v| v.contains('\0')) {
        return Err(format!("{context}: environment values cannot contain NUL"));
    }
    for key in unset {
        if !seen.insert(key) {
            return Err(format!("{context}: duplicate unset_env entry `{key}`"));
        }
        if env.contains_key(key) {
            return Err(format!(
                "{context}: `{key}` appears in both env and unset_env"
            ));
        }
    }
    Ok(())
}

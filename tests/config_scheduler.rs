use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Duration,
};
use takt::{
    config::{self, Format},
    graph,
    model::*,
    scheduler::*,
};

fn parse(source: &str) -> Config {
    config::parse(source, Format::Json, Path::new("/project")).unwrap()
}
fn base() -> Config {
    parse(r#"{"version":1,"tasks":{"a":{"kind":"task","command":["true"]}}}"#)
}
fn task(cpu: u64, memory: u64) -> Task {
    let mut t = Task::command(vec!["true".into()]);
    t.resources = ResourceSpec {
        cpu: Some(Cpu(cpu)),
        memory: Some(Memory(memory)),
    };
    t
}
fn budget(jobs: usize, cpu: u64, memory: u64) -> Budget {
    Budget {
        jobs,
        cpu,
        memory: Some(memory),
        reserve_memory: 0,
        queue_timeout: Some(Duration::from_secs(120)),
    }
}
fn scheduler(config: &Config, budget: Budget) -> Scheduler {
    let names = config.tasks.keys().cloned().collect::<Vec<_>>();
    Scheduler::new(config, graph::select(config, &names).unwrap(), budget).unwrap()
}
fn launch_ids(actions: &[Action]) -> Vec<&str> {
    actions
        .iter()
        .filter_map(|a| {
            if let Action::Launch(id) = a {
                Some(id.as_str())
            } else {
                None
            }
        })
        .collect()
}
fn finish(s: &mut Scheduler, id: &str, code: i32) {
    let outcome = Outcome::exit(code, &s.tasks[id]);
    s.completed(id, outcome).unwrap();
}

#[test]
fn yaml_json_equivalence_and_all_examples() {
    let yaml = config::load(Path::new("examples/01-checks.yaml")).unwrap();
    let json = config::load(Path::new("examples/01-checks.json")).unwrap();
    assert_eq!(yaml, json);
    for item in std::fs::read_dir("examples").unwrap() {
        let path = item.unwrap().path();
        if path.extension().is_some_and(|x| x == "yaml" || x == "json") {
            config::load(&path).unwrap();
        }
    }
}
#[test]
fn rejects_duplicate_json_keys_at_every_depth() {
    for source in [
        r#"{"version":1,"version":1,"tasks":{}}"#,
        r#"{"version":1,"tasks":{"a":{"kind":"task","command":["true"]},"a":{"kind":"task","command":["false"]}}}"#,
        r#"{"version":1,"tasks":{"a":{"kind":"task","command":["true"],"env":{"A":"1","A":"2"}}}}"#,
    ] {
        assert!(
            config::parse(source, Format::Json, Path::new("."))
                .unwrap_err()
                .contains("duplicate")
        );
    }
}

#[test]
fn null_does_not_silently_reset_optional_settings() {
    for source in [
        r#"{"version":1,"scheduling":{"cpu":null},"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
        r#"{"version":1,"tasks":{"a":{"kind":"task","command":["true"],"timeout":null}}}"#,
    ] {
        assert!(config::parse(source, Format::Json, Path::new(".")).is_err());
    }
    assert!(
        config::parse(
            "version: 1\ntasks:\n  a:\n    kind: task\n    command: ['true']\n    timeout: null\n",
            Format::Yaml,
            Path::new(".")
        )
        .is_err()
    );
}

#[test]
fn explicit_auto_and_default_valued_policies_keep_file_provenance() {
    let sources = [
        (
            Format::Json,
            r#"{"version":1,"enforcement":"auto","output":"auto","failure":"continue","service_failure":"continue","scheduling":{"jobs":"auto","cpu":"auto","memory":"auto","queue_timeout":"off"},"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
        ),
        (
            Format::Yaml,
            "version: 1\nenforcement: auto\noutput: auto\nfailure: continue\nservice_failure: continue\nscheduling: {jobs: auto, cpu: auto, memory: auto, queue_timeout: off}\ntasks: {a: {kind: task, command: ['true']}}\n",
        ),
    ];
    for (format, source) in sources {
        let (_, provenance) = config::parse_with_sources(source, format, Path::new(".")).unwrap();
        for key in [
            "enforcement",
            "output",
            "failure",
            "service-failure",
            "jobs",
            "cpu",
            "memory",
            "queue-timeout",
        ] {
            assert_eq!(
                provenance.get(key).map(String::as_str),
                Some("configuration"),
                "{key}"
            );
        }
        assert!(!provenance.contains_key("reserve-memory"));
    }
    let (_, sources) = config::parse_with_sources(
        r#"{"version":1,"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
        Format::Json,
        Path::new("."),
    )
    .unwrap();
    assert!(sources.is_empty());
}

#[test]
fn cpu_precision_is_checked_before_float_rounding() {
    for number in ["1.00000000000000001", "0.0010000000000000001", "1e-4"] {
        let json = format!(
            r#"{{"version":1,"scheduling":{{"cpu":{number}}},"tasks":{{"a":{{"kind":"task","command":["true"]}}}}}}"#
        );
        assert!(
            config::parse(&json, Format::Json, Path::new(".")).is_err(),
            "{number}"
        );
        let yaml = format!(
            "version: 1\nscheduling: {{cpu: {number}}}\ntasks: {{a: {{kind: task, command: ['true']}}}}"
        );
        assert!(
            config::parse(&yaml, Format::Yaml, Path::new(".")).is_err(),
            "{number}"
        );
    }
    let json = r#"{"version":1,"scheduling":{"cpu":1e-3},"tasks":{"a":{"kind":"task","command":["true"]}}}"#;
    assert_eq!(parse(json).scheduling.cpu, Some(Auto::Value(Cpu(1))));
}
#[test]
fn rejects_yaml_duplicates_tags_and_expansion_bombs() {
    for source in [
        "version: 1\nversion: 1\ntasks: {}",
        "version: !!int 1\ntasks: {}",
        "version: 1\ntasks: !local {}",
        "version: 1\ntasks: { a: {kind: task, shell: 'true'}, a: {kind: task, shell: 'false'} }",
    ] {
        assert!(
            config::parse(source, Format::Yaml, Path::new(".")).is_err(),
            "{source}"
        );
    }
    let mut source = String::from("version: 1\ndefaults: &defaults {env: {A: value}}\ntasks:\n");
    for n in 0..1025 {
        source.push_str(&format!("  a{n}: *defaults\n"));
    }
    assert!(config::parse(&source, Format::Yaml, Path::new(".")).is_err());
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert!(config::parse(&deep, Format::Yaml, Path::new(".")).is_err());
    assert!(config::parse(&deep, Format::Json, Path::new(".")).is_err());
    assert!(
        config::parse(
            &" ".repeat(config::MAX_INPUT_BYTES + 1),
            Format::Yaml,
            Path::new(".")
        )
        .is_err()
    );
}

#[test]
fn yaml_aliases_work_within_the_expanded_event_budget() {
    let config=config::parse("version: 1\ndefaults:\n  env: &shared {VALUE: text}\ntasks:\n  a: {kind: task, command: ['true'], env: *shared}\n",Format::Yaml,Path::new(".")).unwrap();
    assert_eq!(config.tasks["a"].env["VALUE"], "text");
    let mut source = String::from("version: 1\ndefaults:\n  env: &shared\n");
    for index in 0..500 {
        source.push_str(&format!("    VALUE{index}: text\n"));
    }
    source.push_str("tasks:\n");
    for index in 0..150 {
        source.push_str(&format!(
            "  task{index}: {{kind: task, command: ['true'], env: *shared}}\n"
        ));
    }
    assert!(source.len() < config::MAX_INPUT_BYTES);
    let error = config::parse(&source, Format::Yaml, Path::new(".")).unwrap_err();
    assert!(
        error.contains("event") || error.contains("budget"),
        "{error}"
    );
}
#[test]
fn rejects_invalid_units_versions_unknown_fields() {
    for cpu in ["0", "-1", "0.0001", "1.1234", "18446744073709551615"] {
        assert!(cpu.parse::<Cpu>().is_err(), "{cpu}");
    }
    assert_eq!("0.125".parse::<Cpu>().unwrap(), Cpu(125));
    for memory in [
        "0B",
        "01B",
        "-1GiB",
        "1GB",
        "1.5GiB",
        "18446744073709551615TiB",
    ] {
        assert!(memory.parse::<Memory>().is_err(), "{memory}");
    }
    for duration in ["0s", "-1m", "1.2s", "2d", "18446744073709551615h"] {
        assert!(duration.parse::<DurationValue>().is_err(), "{duration}");
    }
    for source in [
        r#"{"version":2,"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
        r#"{"version":1,"oops":true,"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
        r#"{"version":1,"scheduling":{"jobs":0},"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
    ] {
        assert!(config::parse(source, Format::Json, Path::new(".")).is_err());
    }
}
#[test]
fn default_dimensions_and_env_merge_independently() {
    let config = parse(
        r#"{"version":1,"defaults":{"resources":{"cpu":2,"memory":"1GiB"},"limits":{"cpu":3,"memory":"2GiB"},"timeout":"5s","env":{"A":"old","B":"keep","REMOVE":"yes"},"unset_env":["RESTORE"]},"tasks":{"a":{"kind":"task","command":["printf","a b","$literal"],"cwd":"sub","resources":{"cpu":0.5},"limits":{"memory":"1GiB"},"timeout":"off","env":{"A":"new","RESTORE":"yes"},"unset_env":["REMOVE"]}}}"#,
    );
    let t = &config.tasks["a"];
    assert_eq!(
        t.resources,
        ResourceSpec {
            cpu: Some(Cpu(500)),
            memory: Some(Memory(1 << 30))
        }
    );
    assert_eq!(
        t.limits,
        ResourceSpec {
            cpu: Some(Cpu(3000)),
            memory: Some(Memory(1 << 30))
        }
    );
    assert_eq!(t.timeout, Some(Timeout::Off));
    assert_eq!(t.cwd.as_deref(), Some(Path::new("/project/sub")));
    assert_eq!(t.env.get("A").map(String::as_str), Some("new"));
    assert!(t.env.contains_key("B"));
    assert!(t.env.contains_key("RESTORE"));
    assert!(!t.env.contains_key("REMOVE"));
    assert_eq!(t.unset_env, ["REMOVE"]);
    assert_eq!(t.command.as_ref().unwrap()[1..], ["a b", "$literal"]);
}
#[test]
fn reject_conflicting_environment_without_printing_values() {
    let err=config::parse(r#"{"version":1,"tasks":{"a":{"kind":"task","command":["true"],"env":{"SECRET":"sensitive-value"},"unset_env":["SECRET"]}}}"#,Format::Json,Path::new(".")).unwrap_err();
    assert!(!err.contains("sensitive-value"));
    assert!(err.contains("both env and unset_env"));
}
#[test]
fn discovery_requires_an_unambiguous_local_file() {
    let dir = tempfile::tempdir().unwrap();
    assert!(config::discover(dir.path(), None).is_err());
    std::fs::write(dir.path().join("takt.yaml"), "").unwrap();
    assert_eq!(
        config::discover(dir.path(), None).unwrap(),
        dir.path().join("takt.yaml")
    );
    std::fs::write(dir.path().join("takt.json"), "").unwrap();
    assert!(config::discover(dir.path(), None).is_err());
    assert_eq!(
        config::discover(dir.path(), Some(Path::new("custom.yaml"))).unwrap(),
        dir.path().join("custom.yaml")
    );
}
#[test]
fn graph_rejects_invalid_edges_and_reports_cycle_path() {
    let mut config = base();
    config.tasks.insert("b".into(), task(1000, 100));
    config.tasks.get_mut("a").unwrap().depends_on = vec![Dependency::Exit {
        task: "b".into(),
        codes: vec![0],
    }];
    config.tasks.get_mut("b").unwrap().depends_on = vec![Dependency::Exit {
        task: "a".into(),
        codes: vec![0],
    }];
    let err = graph::validate(&config).unwrap_err();
    assert!(err.contains("a -> b -> a"));
    config.tasks.get_mut("b").unwrap().depends_on.clear();
    config
        .tasks
        .get_mut("a")
        .unwrap()
        .depends_on
        .push(Dependency::Exit {
            task: "b".into(),
            codes: vec![2],
        });
    assert!(graph::validate(&config).unwrap_err().contains("duplicate"));
    config.tasks.get_mut("a").unwrap().depends_on = vec![Dependency::Ready { task: "b".into() }];
    assert!(graph::validate(&config).unwrap_err().contains("finite"));
    config.tasks.get_mut("b").unwrap().kind = TaskKind::Service;
    assert!(
        graph::validate(&config)
            .unwrap_err()
            .contains("readiness probe")
    );
}
#[test]
fn shared_closure_is_selected_once() {
    let mut config = base();
    for id in ["b", "c"] {
        let mut t = task(1000, 100);
        t.depends_on.push(Dependency::Exit {
            task: "a".into(),
            codes: vec![0],
        });
        config.tasks.insert(id.into(), t);
    }
    config
        .groups
        .insert("all".into(), vec!["b".into(), "c".into()]);
    let selected = graph::select(&config, &["all".into(), "b".into()]).unwrap();
    assert_eq!(selected.tasks.len(), 3);
    assert_eq!(selected.roots.len(), 2);
    assert!(graph::select(&config, &[]).is_err());
    assert!(graph::select(&config, &["missing".into()]).is_err());
}

#[test]
fn environment_default_expansion_has_separate_entry_and_byte_budgets() {
    let tasks: BTreeMap<_, _> = (0..100)
        .map(|index| {
            (
                format!("t{index:03}"),
                serde_json::json!({"kind":"task","command":["true"]}),
            )
        })
        .collect();
    let many_entries: BTreeMap<_, _> = (0..1_001)
        .map(|index| (format!("V{index:04}"), "x"))
        .collect();
    for (defaults, expected) in [
        (
            serde_json::json!({"env":many_entries}),
            "100,000 retained environment/unset entries",
        ),
        (
            serde_json::json!({"env":{"SECRET":"private-value".repeat(12_000)}}),
            "8 MiB of retained environment names/values",
        ),
    ] {
        let source = serde_json::json!({"version":1,"defaults":defaults,"tasks":tasks}).to_string();
        assert!(source.len() < config::MAX_INPUT_BYTES);
        let error = config::parse(&source, Format::Json, Path::new("."))
            .expect_err("small input cannot expand into unbounded inherited environments");
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("reduce inherited environment data"));
        assert!(!error.contains("private-value"));
    }
}

#[test]
fn overridden_or_removed_defaults_do_not_consume_expansion_budget() {
    let tasks: BTreeMap<_, _> = (0..100)
        .map(|index| {
            let mut task = serde_json::json!({"kind":"task","command":["true"]});
            if index % 2 == 0 {
                task["env"] = serde_json::json!({"LARGE":"replacement"});
            } else {
                task["unset_env"] = serde_json::json!(["LARGE"]);
            }
            (format!("t{index:03}"), task)
        })
        .collect();
    let source = serde_json::json!({
        "version":1,"defaults":{"env":{"LARGE":"x".repeat(128 * 1024)}},"tasks":tasks
    })
    .to_string();
    let config = config::parse(&source, Format::Json, Path::new(".")).unwrap();
    assert_eq!(config.tasks["t000"].env["LARGE"], "replacement");
    assert!(config.tasks["t001"].env.is_empty());
    assert_eq!(config.tasks["t001"].unset_env, ["LARGE"]);
}

#[test]
fn service_ancestry_expansion_is_bounded_before_scheduling() {
    let mut config = base();
    config.tasks.clear();
    for index in 0..450 {
        let mut service = task(1, 1);
        service.kind = TaskKind::Service;
        service.readiness = Some(Probe::Stdio {
            stream: Stream::Stdout,
            contains: "READY".into(),
            timeout: None,
        });
        if index > 0 {
            service.depends_on.push(Dependency::Ready {
                task: format!("s{:04}", index - 1),
            });
        }
        config.tasks.insert(format!("s{index:04}"), service);
    }
    graph::validate(&config).unwrap();
    let selected = graph::select(&config, &["s0449".into()]).unwrap();
    let error = Scheduler::new(&config, selected, budget(500, 500, 500))
        .err()
        .expect("service ancestry would otherwise retain over 100,000 entries");
    assert!(error.contains("100,000 retained service-ancestor entries"));
    assert!(error.contains("select fewer tasks or split this run"));
}

#[test]
fn long_finite_dependency_chains_do_not_accumulate_service_ancestry() {
    let mut config = base();
    config.tasks.clear();
    for index in 0..1_600 {
        let mut finite = task(1, 1);
        if index > 0 {
            finite.depends_on.push(Dependency::Exit {
                task: format!("t{:04}", index - 1),
                codes: vec![0],
            });
        }
        config.tasks.insert(format!("t{index:04}"), finite);
    }
    graph::validate(&config).unwrap();
    let selected = graph::select(&config, &["t1599".into()]).unwrap();
    let ancestry = graph::service_ancestry(&config, &selected.tasks).unwrap();
    assert_eq!(ancestry.len(), 1_600);
    assert!(ancestry.values().all(BTreeSet::is_empty));
    let mut scheduler = Scheduler::new(&config, selected, budget(1, 1, 1)).unwrap();
    assert_eq!(
        launch_ids(&scheduler.tick(Duration::ZERO, &Snapshot::default())),
        ["t0000"]
    );
}

#[test]
fn only_inferred_requests_shrink() {
    let config = base();
    let s = scheduler(&config, budget(1, 250, 64 << 20));
    assert_eq!(s.requests["a"].cpu, 250);
    assert_eq!(s.requests["a"].memory, 64 << 20);
    let config = parse(
        r#"{"version":1,"defaults":{"resources":{"cpu":1}},"tasks":{"a":{"kind":"task","command":["true"]}}}"#,
    );
    assert!(
        Scheduler::new(
            &config,
            graph::select(&config, &["a".into()]).unwrap(),
            budget(1, 250, 64 << 20)
        )
        .is_err()
    );
}
#[test]
fn reservations_hold_until_cleanup_and_cannot_release_twice() {
    let mut config = base();
    config.tasks = BTreeMap::from([("a".into(), task(1000, 100)), ("b".into(), task(1000, 100))]);
    let mut s = scheduler(&config, budget(1, 1000, 100));
    assert_eq!(
        launch_ids(&s.tick(Duration::ZERO, &Snapshot::default())),
        ["a"]
    );
    assert!(
        s.tick(Duration::from_secs(1), &Snapshot::default())
            .is_empty()
    );
    assert_eq!(s.reservations(), (1, 1000, 100));
    s.started("a").unwrap();
    s.states.insert("a".into(), State::Stopping);
    assert!(
        s.tick(Duration::from_secs(2), &Snapshot::default())
            .is_empty()
    );
    finish(&mut s, "a", 0);
    assert!(s.completed("a", Outcome::stopped()).is_err());
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(3), &Snapshot::default())),
        ["b"]
    );
}
#[test]
fn live_memory_counts_only_unconsumed_reservations() {
    let mut config = base();
    config.tasks = BTreeMap::from([("a".into(), task(1000, 100)), ("b".into(), task(1000, 100))]);
    let mut s = scheduler(&config, budget(2, 2000, 200));
    let snapshot = Snapshot {
        available_memory: Some(150),
        usage: BTreeMap::new(),
    };
    assert_eq!(launch_ids(&s.tick(Duration::ZERO, &snapshot)), ["a"]);
    assert!(matches!(
        s.states["b"],
        State::Queued {
            reason: QueueReason::LiveMemoryPressure,
            ..
        }
    ));
    let snapshot = Snapshot {
        available_memory: Some(120),
        usage: BTreeMap::from([("a".into(), 80)]),
    };
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(1), &snapshot)),
        ["b"]
    );
}

#[test]
fn lowering_a_budget_pauses_admission_without_stopping_healthy_tasks() {
    let mut config = base();
    config.tasks = BTreeMap::from([
        ("a".into(), task(1000, 100)),
        ("b".into(), task(1000, 100)),
        ("c".into(), task(1000, 100)),
    ]);
    let mut s = scheduler(&config, budget(2, 2000, 200));
    assert_eq!(
        launch_ids(&s.tick(Duration::ZERO, &Snapshot::default())),
        ["a", "b"]
    );
    s.budget.jobs = 1;
    s.budget.cpu = 1000;
    s.budget.memory = Some(100);
    assert!(
        s.tick(Duration::from_secs(1), &Snapshot::default())
            .is_empty()
    );
    assert_eq!(s.reservations(), (2, 2000, 200));
    finish(&mut s, "a", 0);
    assert!(
        s.tick(Duration::from_secs(2), &Snapshot::default())
            .is_empty()
    );
    finish(&mut s, "b", 0);
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(3), &Snapshot::default())),
        ["c"]
    );
}
#[test]
fn later_fitting_tasks_pass_until_fairness_ages() {
    let mut config = base();
    config.tasks = BTreeMap::from([
        ("a".into(), task(1000, 10)),
        ("b".into(), task(2000, 10)),
        ("c".into(), task(1000, 10)),
        ("d".into(), task(1000, 10)),
    ]);
    let mut s = scheduler(&config, budget(3, 2000, 100));
    assert_eq!(
        launch_ids(&s.tick(Duration::ZERO, &Snapshot::default())),
        ["a", "c"]
    );
    finish(&mut s, "c", 0);
    assert!(launch_ids(&s.tick(Duration::from_secs(6), &Snapshot::default())).is_empty());
    finish(&mut s, "a", 0);
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(7), &Snapshot::default())),
        ["b"]
    );
}
#[test]
fn execution_failure_and_edge_matching_are_separate() {
    let mut config = base();
    let mut b = task(1000, 100);
    b.depends_on.push(Dependency::Exit {
        task: "a".into(),
        codes: vec![2],
    });
    let mut c = b.clone();
    c.depends_on = vec![Dependency::Exit {
        task: "a".into(),
        codes: vec![0],
    }];
    config.tasks.insert("b".into(), b);
    config.tasks.insert("c".into(), c);
    let mut s = scheduler(&config, budget(3, 3000, 3 << 30));
    s.tick(Duration::ZERO, &Snapshot::default());
    finish(&mut s, "a", 2);
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(1), &Snapshot::default())),
        ["b"]
    );
    assert!(s.any_failed());
    assert_eq!(s.states["c"].outcome().unwrap().kind, OutcomeKind::Blocked);
    config.tasks.get_mut("a").unwrap().success_codes = Some(vec![0, 2]);
    let mut s = scheduler(&config, budget(3, 3000, 3 << 30));
    s.tick(Duration::ZERO, &Snapshot::default());
    finish(&mut s, "a", 2);
    s.tick(Duration::from_secs(1), &Snapshot::default());
    assert_eq!(s.states["c"].outcome().unwrap().kind, OutcomeKind::Skipped);
    assert!(!s.any_failed());
}

#[test]
fn neutral_skips_propagate_through_the_selected_dependency_closure() {
    let mut config = base();
    config.tasks.get_mut("a").unwrap().success_codes = Some(vec![0, 2]);
    let mut branch = task(1000, 100);
    branch.depends_on.push(Dependency::Exit {
        task: "a".into(),
        codes: vec![0],
    });
    let mut leaf = task(1000, 100);
    leaf.depends_on.push(Dependency::Exit {
        task: "branch".into(),
        codes: vec![0],
    });
    config.tasks.insert("branch".into(), branch);
    config.tasks.insert("leaf".into(), leaf);
    let selection = graph::select(&config, &["leaf".into()]).unwrap();
    let mut s = Scheduler::new(&config, selection, budget(1, 1000, 1 << 30)).unwrap();
    s.tick(Duration::ZERO, &Snapshot::default());
    finish(&mut s, "a", 2);
    let actions = s.tick(Duration::from_secs(1), &Snapshot::default());
    assert!(launch_ids(&actions).is_empty());
    assert_eq!(
        s.states["branch"].outcome().unwrap().kind,
        OutcomeKind::Skipped
    );
    assert_eq!(
        s.states["leaf"].outcome().unwrap().kind,
        OutcomeKind::Skipped
    );
    assert!(s.is_done());
    assert!(!s.any_failed());
}
#[test]
fn nonexit_outcome_never_matches_exit_predicate() {
    let mut config = base();
    let mut b = task(1000, 100);
    b.depends_on.push(Dependency::Exit {
        task: "a".into(),
        codes: vec![0],
    });
    config.tasks.insert("b".into(), b);
    for reason in [
        Reason::Signal,
        Reason::ExecutionTimeout,
        Reason::ReadinessTimeout,
        Reason::MemoryLimit,
        Reason::SpawnFailure,
    ] {
        let mut s = scheduler(&config, budget(2, 2000, 2 << 30));
        s.tick(Duration::ZERO, &Snapshot::default());
        let mut outcome = Outcome::failed(reason);
        outcome.exit_code = Some(0);
        s.completed("a", outcome).unwrap();
        assert!(launch_ids(&s.tick(Duration::from_secs(1), &Snapshot::default())).is_empty());
        assert_eq!(s.states["b"].outcome().unwrap().kind, OutcomeKind::Blocked);
    }
}

#[test]
fn successful_exit_dependency_waits_for_descendant_cleanup() {
    let mut config = base();
    let mut dependent = task(1000, 100);
    dependent.depends_on.push(Dependency::Exit {
        task: "a".into(),
        codes: vec![0],
    });
    config.tasks.insert("b".into(), dependent);
    let mut s = scheduler(&config, budget(2, 2000, 2 << 30));
    s.tick(Duration::ZERO, &Snapshot::default());
    s.states.insert("a".into(), State::Stopping);
    assert!(
        s.tick(Duration::from_secs(1), &Snapshot::default())
            .is_empty()
    );
    assert_eq!(s.states["b"], State::Pending);
    finish(&mut s, "a", 0);
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(2), &Snapshot::default())),
        ["b"]
    );
}

#[test]
fn fail_fast_observes_failure_before_descendant_cleanup() {
    let mut config = base();
    config.failure = FailurePolicy::FailFast;
    config.tasks = BTreeMap::from([("a".into(), task(1000, 100)), ("b".into(), task(1000, 100))]);
    let mut s = scheduler(&config, budget(1, 1000, 100));
    s.tick(Duration::ZERO, &Snapshot::default());
    let outcome = Outcome::exit(2, &s.tasks["a"]);
    s.stopping("a", &outcome).unwrap();
    s.stopping("a", &outcome).unwrap();
    assert!(s.any_failed());
    assert_eq!(s.reservations(), (1, 1000, 100));
    assert!(launch_ids(&s.tick(Duration::from_secs(1), &Snapshot::default())).is_empty());
    assert_eq!(s.states["b"].outcome().unwrap().reason, Reason::FailFast);
    s.completed("a", outcome.clone()).unwrap();
    assert_eq!(s.reservations(), (0, 0, 0));
    assert!(s.stopping("a", &outcome).is_err());
    assert!(s.started("a").is_err());
}
#[test]
fn resource_deadline_does_not_count_dependency_wait() {
    let mut config = base();
    let mut b = task(1000, 100);
    b.depends_on.push(Dependency::Exit {
        task: "a".into(),
        codes: vec![0],
    });
    config.tasks.insert("b".into(), b);
    let mut s = scheduler(&config, budget(1, 1000, 1 << 30));
    s.tick(Duration::ZERO, &Snapshot::default());
    assert!(
        s.tick(Duration::from_secs(300), &Snapshot::default())
            .is_empty()
    );
    assert_eq!(s.states["b"], State::Pending);
    finish(&mut s, "a", 0);
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(301), &Snapshot::default())),
        ["b"]
    );
    let mut s = scheduler(&base(), budget(1, 1000, 1 << 30));
    s.tick(
        Duration::ZERO,
        &Snapshot {
            available_memory: Some(1),
            usage: BTreeMap::new(),
        },
    );
    s.tick(Duration::from_secs(120), &Snapshot::default());
    assert_eq!(
        s.states["a"].outcome().unwrap().reason,
        Reason::QueueTimeout
    );
}
fn services_config() -> Config {
    parse(
        r#"{"version":1,"tasks":{"api":{"kind":"service","command":["service"],"resources":{"cpu":1,"memory":"100B"},"readiness":{"type":"stdio","stream":"stdout","contains":"READY"}},"one":{"kind":"task","command":["true"],"resources":{"cpu":1,"memory":"100B"},"depends_on":[{"task":"api","when":"ready"}]},"two":{"kind":"task","command":["true"],"resources":{"cpu":1,"memory":"100B"},"depends_on":[{"task":"api","when":"ready"}]}}}"#,
    )
}
#[test]
fn temporary_shared_service_survives_all_consumers() {
    let config = services_config();
    let selected = graph::select(&config, &["one".into(), "two".into()]).unwrap();
    let mut s = Scheduler::new(&config, selected, budget(2, 2000, 200)).unwrap();
    assert_eq!(
        launch_ids(&s.tick(Duration::ZERO, &Snapshot::default())),
        ["api"]
    );
    s.started("api").unwrap();
    assert!(
        s.tick(Duration::from_secs(1), &Snapshot::default())
            .is_empty()
    );
    s.ready("api").unwrap();
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(2), &Snapshot::default())),
        ["one"]
    );
    finish(&mut s, "one", 0);
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(3), &Snapshot::default())),
        ["two"]
    );
    finish(&mut s, "two", 0);
    assert_eq!(
        s.tick(Duration::from_secs(4), &Snapshot::default()),
        [Action::Stop("api".into(), Reason::ServiceUnused)]
    );
    assert!(!s.is_done());
    s.completed("api", Outcome::stopped()).unwrap();
    assert!(s.is_done());
}

#[test]
fn skipped_root_does_not_tear_down_its_active_prerequisite_service() {
    let mut config = services_config();
    let mut detect = task(1000, 100);
    detect.success_codes = Some(vec![2]);
    config.tasks.insert("detect".into(), detect);
    let mut root = task(1000, 100);
    root.depends_on = vec![
        Dependency::Exit {
            task: "detect".into(),
            codes: vec![0],
        },
        Dependency::Exit {
            task: "one".into(),
            codes: vec![0],
        },
    ];
    config.tasks.insert("root".into(), root);
    let selected = graph::select(&config, &["root".into()]).unwrap();
    let mut s = Scheduler::new(&config, selected, budget(3, 3000, 300)).unwrap();
    s.tick(Duration::ZERO, &Snapshot::default());
    s.started("api").unwrap();
    finish(&mut s, "detect", 2);
    let actions = s.tick(Duration::from_secs(1), &Snapshot::default());
    assert_eq!(
        s.states["root"].outcome().unwrap().kind,
        OutcomeKind::Skipped
    );
    assert!(
        !actions
            .iter()
            .any(|a| matches!(a,Action::Stop(id,_) if id=="api"))
    );
    s.ready("api").unwrap();
    assert_eq!(
        launch_ids(&s.tick(Duration::from_secs(2), &Snapshot::default())),
        ["one"]
    );
    finish(&mut s, "one", 0);
    assert_eq!(
        s.tick(Duration::from_secs(3), &Snapshot::default()),
        [Action::Stop("api".into(), Reason::ServiceUnused)]
    );
}
#[test]
fn service_reservation_deadlocks_are_diagnosed() {
    let config = services_config();
    let selected = graph::select(&config, &["one".into()]).unwrap();
    assert!(
        Scheduler::new(&config, selected, budget(1, 2000, 200))
            .err()
            .unwrap()
            .contains("service-reservation deadlock")
    );
    let mut config = base();
    let mut service = task(1000, 100);
    service.kind = TaskKind::Service;
    config.tasks = BTreeMap::from([
        ("a_service".into(), service),
        ("b_finite".into(), task(1000, 100)),
    ]);
    let mut s = scheduler(&config, budget(1, 1000, 100));
    s.tick(Duration::ZERO, &Snapshot::default());
    s.started("a_service").unwrap();
    s.tick(Duration::from_secs(1), &Snapshot::default());
    assert_eq!(
        s.states["b_finite"].outcome().unwrap().reason,
        Reason::ServiceDeadlock
    );
}
#[test]
fn failed_service_blocks_pending_but_allows_active_consumers() {
    let config = services_config();
    let selected = graph::select(&config, &["one".into(), "two".into()]).unwrap();
    let mut s = Scheduler::new(&config, selected, budget(2, 2000, 200)).unwrap();
    s.tick(Duration::ZERO, &Snapshot::default());
    s.started("api").unwrap();
    s.ready("api").unwrap();
    s.tick(Duration::from_secs(1), &Snapshot::default());
    s.completed("api", Outcome::failed(Reason::ServiceFailed))
        .unwrap();
    s.tick(Duration::from_secs(2), &Snapshot::default());
    assert!(s.states["one"].is_active());
    assert_eq!(
        s.states["two"].outcome().unwrap().kind,
        OutcomeKind::Blocked
    );
}

#[test]
fn stop_dependents_cancels_only_the_transitive_service_consumers() {
    let mut config = services_config();
    config.service_failure = ServiceFailurePolicy::StopDependents;
    config.tasks.insert("independent".into(), task(1000, 100));
    let mut three = task(1000, 100);
    three.depends_on.push(Dependency::Exit {
        task: "one".into(),
        codes: vec![0],
    });
    config.tasks.insert("three".into(), three);
    let selection = graph::select(
        &config,
        &[
            "one".into(),
            "two".into(),
            "three".into(),
            "independent".into(),
        ],
    )
    .unwrap();
    let mut s = Scheduler::new(&config, selection, budget(4, 4000, 400)).unwrap();
    s.tick(Duration::ZERO, &Snapshot::default());
    s.started("api").unwrap();
    s.ready("api").unwrap();
    s.tick(Duration::from_secs(1), &Snapshot::default());
    s.completed("api", Outcome::failed(Reason::ServiceFailed))
        .unwrap();
    let actions = s.tick(Duration::from_secs(2), &Snapshot::default());
    assert!(actions.contains(&Action::Stop("one".into(), Reason::ServiceFailed)));
    assert!(actions.contains(&Action::Stop("two".into(), Reason::ServiceFailed)));
    assert_eq!(
        s.states["three"].outcome().unwrap().kind,
        OutcomeKind::Cancelled
    );
    assert!(s.states["independent"].is_active());
}

#[test]
fn service_failure_cancels_consumers_before_provider_cleanup_once() {
    let mut config = services_config();
    config.service_failure = ServiceFailurePolicy::StopDependents;
    let selection = graph::select(&config, &["one".into(), "two".into()]).unwrap();
    let mut s = Scheduler::new(&config, selection, budget(3, 3000, 300)).unwrap();
    s.tick(Duration::ZERO, &Snapshot::default());
    s.started("api").unwrap();
    s.ready("api").unwrap();
    s.tick(Duration::from_secs(1), &Snapshot::default());
    let failure = Outcome::failed(Reason::ServiceFailed);
    s.stopping("api", &failure).unwrap();
    s.stopping("api", &failure).unwrap();
    assert_eq!(s.reservations(), (3, 3000, 300));
    let actions = s.tick(Duration::from_secs(2), &Snapshot::default());
    assert!(actions.contains(&Action::Stop("one".into(), Reason::ServiceFailed)));
    assert!(actions.contains(&Action::Stop("two".into(), Reason::ServiceFailed)));
    assert_eq!(actions.len(), 2);
    s.completed("api", failure).unwrap();
    assert!(
        s.tick(Duration::from_secs(3), &Snapshot::default())
            .is_empty()
    );
    assert_eq!(s.reservations(), (2, 2000, 200));
    assert!(s.ready("api").is_err());
}
#[test]
fn fail_fast_cancels_continuations_even_when_edge_matches() {
    let mut config = base();
    config.failure = FailurePolicy::FailFast;
    let mut b = task(1000, 100);
    b.depends_on.push(Dependency::Exit {
        task: "a".into(),
        codes: vec![2],
    });
    config.tasks.insert("b".into(), b);
    let mut s = scheduler(&config, budget(2, 2000, 2 << 30));
    s.tick(Duration::ZERO, &Snapshot::default());
    finish(&mut s, "a", 2);
    assert!(launch_ids(&s.tick(Duration::from_secs(1), &Snapshot::default())).is_empty());
    assert_eq!(
        s.states["b"].outcome().unwrap().kind,
        OutcomeKind::Cancelled
    );
}
#[test]
fn probe_validation_is_strict() {
    for url in [
        "https://example.com/health",
        "http://",
        "http://bad host/",
        "http://localhost:0/",
        "http://localhost:65536/",
        "http://[::1/",
    ] {
        assert!(graph::validate_http_url(url).is_err(), "{url}");
    }
    for url in [
        "http://localhost/health",
        "http://127.0.0.1:8080/",
        "http://[::1]:8080/health?x=1",
    ] {
        graph::validate_http_url(url).unwrap();
    }
    assert_eq!(
        Bytes::Hex("00ff504f4e47".into()).decode().unwrap(),
        [0, 255, 80, 79, 78, 71]
    );
    assert!(Bytes::Hex("x1".into()).decode().is_err());
    assert!(Bytes::Text("".into()).decode().is_err());
}

proptest::proptest! {
    #[test]
    fn generated_dags_launch_only_after_matching_prerequisites_and_settle(
        nodes in proptest::collection::vec((1_u64..4000,1_u64..1024,proptest::num::u16::ANY,proptest::bool::ANY),1..12),
        jobs in 1_usize..6,
    ) {
        let mut config=base();config.tasks.clear();
        for (i,(cpu,memory,edges,success)) in nodes.iter().enumerate() {
            let mut t=task(*cpu,*memory);
            if *success{t.success_codes=Some(vec![0,2]);}
            for parent in 0..i {
                if edges & (1<<parent)!=0 {
                    t.depends_on.push(Dependency::Exit{task:format!("t{parent:02}"),codes:vec![if edges & (1<<15)!=0 {2}else{0}]});
                }
            }
            config.tasks.insert(format!("t{i:02}"),t);
        }
        graph::validate(&config).unwrap();
        let mut s=scheduler(&config,budget(jobs,4000,2048));let mut launched=BTreeSet::new();
        for turn in 0..=nodes.len() {
            let actions=s.tick(Duration::from_millis(turn as u64),&Snapshot::default());
            for id in launch_ids(&actions) {
                proptest::prop_assert!(launched.insert(id.to_owned()));
                for dependency in &s.tasks[id].depends_on {
                    if let Dependency::Exit{task,codes}=dependency {
                        let outcome=s.states[task].outcome().expect("launched only after cleanup");
                        proptest::prop_assert_eq!(outcome.reason,Reason::NormalExit);
                        proptest::prop_assert!(outcome.exit_code.is_some_and(|c|codes.contains(&(c as u8))));
                    }
                }
            }
            let(active,cpu,memory)=s.reservations();proptest::prop_assert!(active<=jobs);proptest::prop_assert!(cpu<=4000);proptest::prop_assert!(memory<=2048);
            let active:Vec<_>=s.active().into_iter().map(str::to_owned).rev().collect();
            for id in active {let index=id[1..].parse::<usize>().unwrap();finish(&mut s,&id,if nodes[index].2 & 1!=0 {2}else{0});}
            if s.is_done(){break;}
        }
        proptest::prop_assert!(s.is_done());
        proptest::prop_assert_eq!(s.reservations(),(0,0,0));
    }

    #[test]
    fn weighted_admission_never_oversubscribes_or_relaunches(weights in proptest::collection::vec((1_u64..4000,1_u64..1024),1..40),jobs in 1_usize..8){
        let mut config=base();config.tasks.clear();for (i,(cpu,memory)) in weights.iter().enumerate(){config.tasks.insert(format!("t{i:03}"),task(*cpu,*memory));}
        let mut s=scheduler(&config,budget(jobs,4000,2048));let mut launched=BTreeSet::new();
        for turn in 0..weights.len()+1 {
            let actions=s.tick(Duration::from_millis(turn as u64),&Snapshot::default());
            for id in launch_ids(&actions){proptest::prop_assert!(launched.insert(id.to_owned()));}
            let(active,cpu,memory)=s.reservations();proptest::prop_assert!(active<=jobs);proptest::prop_assert!(cpu<=4000);proptest::prop_assert!(memory<=2048);
            let active:Vec<_>=s.active().into_iter().map(str::to_owned).collect();for id in active{finish(&mut s,&id,0);}
            if s.is_done(){break;}
        }
        proptest::prop_assert!(s.is_done());proptest::prop_assert_eq!(launched.len(),weights.len());proptest::prop_assert_eq!(s.reservations(),(0,0,0));
    }
}

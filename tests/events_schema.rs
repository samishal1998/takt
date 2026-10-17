#[path = "support/events.rs"]
mod event_contract;

use serde_json::{Value, json};

fn schema() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(include_str!("../schema/events.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

#[test]
fn schema_accepts_byte_safe_logs_and_rejects_malformed_envelopes() {
    let validator = schema();
    let valid = json!({"schema_version":1,"run_id":"example","sequence":1,"elapsed_ms":0,"event":"log","task_id":"test","stream":"stderr","encoding":"base64","data":"AP8="});
    event_contract::assert_event(&valid);
    for (field, value) in [
        ("schema_version", json!(2)),
        ("sequence", json!(0)),
        ("elapsed_ms", json!(-1)),
        ("encoding", json!("latin1")),
        ("data", json!("bad base64!")),
        ("stream", json!("combined")),
        ("task_id", json!("../escape")),
        ("event", json!("unknown")),
    ] {
        let mut malformed = valid.clone();
        malformed[field] = value;
        assert!(!validator.is_valid(&malformed), "accepted invalid {field}");
    }
    let mut missing_task = valid;
    missing_task.as_object_mut().unwrap().remove("task_id");
    assert!(!validator.is_valid(&missing_task));
}

#[test]
fn outcome_and_summary_contract_preserves_neutral_skips() {
    let outcome = json!({"kind":"skipped","reason":"condition_not_matched","exit_code":null,"signal":null,"detail":null});
    let events = vec![
        json!({"schema_version":1,"run_id":"example","sequence":1,"elapsed_ms":0,"event":"task_finished","task_id":"branch","kind":"skipped","reason":"condition_not_matched","exit_code":null,"signal":null,"detail":null}),
        json!({"schema_version":1,"run_id":"example","sequence":2,"elapsed_ms":1,"event":"summary","selected_roots":["branch"],"outcomes":{"branch":outcome},"counts":{"skipped":1},"exit_code":0}),
    ];
    event_contract::assert_events(&events);
    let mut bad = events[0].clone();
    bad["reason"] = json!("unrecognised_reason");
    assert!(!schema().is_valid(&bad));
    let mut bad = events[1].clone();
    bad["exit_code"] = json!(137);
    assert!(
        !schema().is_valid(&bad),
        "runner codes do not inherit raw child exits"
    );
}

#[test]
fn configuration_examples_match_the_published_schema() {
    let schema: Value = serde_json::from_str(include_str!("../schema/takt.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for entry in std::fs::read_dir("examples").unwrap() {
        let path = entry.unwrap().path();
        let source = std::fs::read_to_string(&path).unwrap();
        let value: Value = match path.extension().and_then(|s| s.to_str()) {
            Some("yaml" | "yml") => serde_saphyr::from_str(&source).unwrap(),
            Some("json") => serde_json::from_str(&source).unwrap(),
            _ => continue,
        };
        if let Err(error) = validator.validate(&value) {
            panic!("{} violates configuration schema: {error}", path.display());
        }
    }
}

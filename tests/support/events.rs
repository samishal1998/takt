use serde_json::Value;
use std::{collections::BTreeMap, sync::OnceLock};

fn validator() -> &'static jsonschema::Validator {
    static VALIDATOR: OnceLock<jsonschema::Validator> = OnceLock::new();
    VALIDATOR.get_or_init(|| {
        let schema: Value = serde_json::from_str(include_str!("../../schema/events.schema.json"))
            .expect("event schema is JSON");
        jsonschema::validator_for(&schema)
            .expect("event schema compiles without external references")
    })
}

pub fn assert_event(event: &Value) {
    if let Err(error) = validator().validate(event) {
        panic!("event violates schema: {error}\nevent: {event}");
    }
}

pub fn assert_events(events: &[Value]) {
    let mut elapsed = 0;
    let mut run_id = None;
    for (index, event) in events.iter().enumerate() {
        assert_event(event);
        assert_eq!(event["sequence"].as_u64(), Some(index as u64 + 1));
        let current_elapsed = event["elapsed_ms"].as_u64().unwrap();
        assert!(
            current_elapsed >= elapsed,
            "elapsed time cannot go backwards"
        );
        elapsed = current_elapsed;
        if let Some(id) = run_id {
            assert_eq!(event["run_id"].as_str(), Some(id));
        } else {
            run_id = event["run_id"].as_str();
        }
        if event["event"] == "summary" {
            let mut counts = BTreeMap::<&str, u64>::new();
            for outcome in event["outcomes"].as_object().unwrap().values() {
                *counts.entry(outcome["kind"].as_str().unwrap()).or_default() += 1;
            }
            let actual: BTreeMap<&str, u64> = event["counts"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(kind, count)| (kind.as_str(), count.as_u64().unwrap()))
                .collect();
            assert_eq!(
                counts, actual,
                "summary counts must account for every outcome"
            );
        }
    }
}

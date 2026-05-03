//! Temporal trace assertion evaluation over timeline events.

use serde_json::json;

use super::parsing::{
    TraceAssertion, TraceAssertionStatus, TraceResults, TraceStatus,
    TraceTemporalAssertionDefinition,
};
use super::spans::{event_matches_key, reporting_timeline};

pub(crate) fn apply_temporal_assertions(results: &mut TraceResults) -> bool {
    if results.temporal_assertions.is_empty() {
        return false;
    }

    let definitions = std::mem::take(&mut results.temporal_assertions);
    let timeline = reporting_timeline(&results.timeline);
    let mut has_failure = false;
    for definition in &definitions {
        let assertion = evaluate_temporal_assertion(definition, &timeline);
        if assertion.status != TraceAssertionStatus::Pass {
            has_failure = true;
        }
        results.assertions.push(assertion);
    }

    if has_failure {
        results.status = TraceStatus::Fail;
    }
    has_failure
}

fn evaluate_temporal_assertion(
    definition: &TraceTemporalAssertionDefinition,
    timeline: &[super::parsing::TraceEvent],
) -> TraceAssertion {
    match definition {
        TraceTemporalAssertionDefinition::Count {
            id,
            events,
            min,
            max,
            message,
        } => evaluate_count(id, events, *min, *max, message.as_deref(), timeline),
        TraceTemporalAssertionDefinition::ForbiddenEvent {
            id,
            pattern,
            message,
        } => evaluate_forbidden_event(id, pattern, message.as_deref(), timeline),
        TraceTemporalAssertionDefinition::MaxConcurrent {
            id,
            track,
            max,
            message,
        } => evaluate_max_concurrent(id, track, *max, message.as_deref(), timeline),
    }
}

fn evaluate_count(
    id: &str,
    events: &[String],
    min: Option<usize>,
    max: Option<usize>,
    message: Option<&str>,
    timeline: &[super::parsing::TraceEvent],
) -> TraceAssertion {
    let matches = timeline
        .iter()
        .filter(|event| events.iter().any(|key| event_matches_key(event, key)))
        .collect::<Vec<_>>();
    let actual = matches.len();
    let passed = min.is_none_or(|value| actual >= value) && max.is_none_or(|value| actual <= value);

    TraceAssertion {
        id: id.to_string(),
        status: if passed {
            TraceAssertionStatus::Pass
        } else {
            TraceAssertionStatus::Fail
        },
        message: Some(
            message
                .map(str::to_string)
                .unwrap_or_else(|| count_message(events, min, max, actual, passed)),
        ),
        details: Some(json!({
            "kind": "count",
            "events": events,
            "min": min,
            "max": max,
            "actual": actual,
            "matches": event_details(&matches),
        })),
    }
}

fn evaluate_forbidden_event(
    id: &str,
    pattern: &str,
    message: Option<&str>,
    timeline: &[super::parsing::TraceEvent],
) -> TraceAssertion {
    let matches = timeline
        .iter()
        .filter(|event| event_matches_key(event, pattern))
        .collect::<Vec<_>>();
    let passed = matches.is_empty();

    TraceAssertion {
        id: id.to_string(),
        status: if passed {
            TraceAssertionStatus::Pass
        } else {
            TraceAssertionStatus::Fail
        },
        message: Some(
            message
                .map(str::to_string)
                .unwrap_or_else(|| forbidden_event_message(pattern, matches.len(), passed)),
        ),
        details: Some(json!({
            "kind": "forbidden-event",
            "pattern": pattern,
            "actual": matches.len(),
            "matches": event_details(&matches),
        })),
    }
}

fn evaluate_max_concurrent(
    id: &str,
    track: &[String],
    max: usize,
    message: Option<&str>,
    timeline: &[super::parsing::TraceEvent],
) -> TraceAssertion {
    if track.len() != 2 {
        return TraceAssertion {
            id: id.to_string(),
            status: TraceAssertionStatus::Error,
            message: Some(
                "max-concurrent requires exactly two track events: start and end".to_string(),
            ),
            details: Some(json!({
                "kind": "max-concurrent",
                "track": track,
                "max": max,
            })),
        };
    }

    let mut changes = timeline
        .iter()
        .filter_map(|event| {
            if event_matches_key(event, &track[0]) {
                Some((event.t_ms, 1_i64, event))
            } else if event_matches_key(event, &track[1]) {
                Some((event.t_ms, -1_i64, event))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    changes.sort_by_key(|(t_ms, delta, _)| (*t_ms, *delta));

    let mut current = 0_i64;
    let mut max_observed = 0_i64;
    let mut max_t_ms = None;
    for (t_ms, delta, _) in &changes {
        current = (current + delta).max(0);
        if current > max_observed {
            max_observed = current;
            max_t_ms = Some(*t_ms);
        }
    }

    let passed = max_observed <= max as i64;
    TraceAssertion {
        id: id.to_string(),
        status: if passed {
            TraceAssertionStatus::Pass
        } else {
            TraceAssertionStatus::Fail
        },
        message: Some(
            message
                .map(str::to_string)
                .unwrap_or_else(|| max_concurrent_message(&track[0], max, max_observed, passed)),
        ),
        details: Some(json!({
            "kind": "max-concurrent",
            "track": track,
            "max": max,
            "max_observed": max_observed,
            "at_t_ms": max_t_ms,
            "events": event_details(&changes.iter().map(|(_, _, event)| *event).collect::<Vec<_>>()),
        })),
    }
}

fn count_message(
    events: &[String],
    min: Option<usize>,
    max: Option<usize>,
    actual: usize,
    passed: bool,
) -> String {
    if passed {
        return format!(
            "event count for `{}` satisfied: observed {actual}",
            events.join(", ")
        );
    }

    match (min, max) {
        (Some(min), Some(max)) => format!(
            "event count for `{}` outside range {min}..={max}: observed {actual}",
            events.join(", ")
        ),
        (Some(min), None) => format!(
            "event count for `{}` below minimum {min}: observed {actual}",
            events.join(", ")
        ),
        (None, Some(max)) => format!(
            "event count for `{}` exceeded maximum {max}: observed {actual}",
            events.join(", ")
        ),
        (None, None) => format!("event count for `{}` observed {actual}", events.join(", ")),
    }
}

fn forbidden_event_message(pattern: &str, actual: usize, passed: bool) -> String {
    if passed {
        format!("forbidden event `{pattern}` did not occur")
    } else {
        format!("forbidden event `{pattern}` occurred {actual} time(s)")
    }
}

fn max_concurrent_message(start: &str, max: usize, max_observed: i64, passed: bool) -> String {
    if passed {
        format!("max concurrency for `{start}` stayed within {max}: observed {max_observed}")
    } else {
        format!("max concurrency for `{start}` exceeded {max}: observed {max_observed}")
    }
}

fn event_details(events: &[&super::parsing::TraceEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .map(|event| {
            json!({
                "t_ms": event.t_ms,
                "source": event.source,
                "event": event.event,
                "data": event.data,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::trace::parsing::TraceEvent;
    use std::collections::BTreeMap;

    fn event(t_ms: u64, source: &str, event: &str) -> TraceEvent {
        TraceEvent {
            t_ms,
            source: source.to_string(),
            event: event.to_string(),
            data: BTreeMap::new(),
        }
    }

    fn results(
        timeline: Vec<TraceEvent>,
        temporal_assertions: Vec<TraceTemporalAssertionDefinition>,
    ) -> TraceResults {
        TraceResults {
            component_id: "example".to_string(),
            scenario_id: "synthetic".to_string(),
            status: TraceStatus::Pass,
            summary: None,
            failure: None,
            rig: None,
            timeline,
            span_definitions: Vec::new(),
            span_results: Vec::new(),
            assertions: Vec::new(),
            temporal_assertions,
            artifacts: Vec::new(),
        }
    }

    #[test]
    fn test_apply_temporal_assertions() {
        let mut results = results(
            vec![event(10, "runner", "ready")],
            vec![TraceTemporalAssertionDefinition::Count {
                id: "ready-once".to_string(),
                events: vec!["runner.ready".to_string()],
                min: Some(1),
                max: Some(1),
                message: None,
            }],
        );

        assert!(!apply_temporal_assertions(&mut results));
        assert_eq!(results.status, TraceStatus::Pass);
        assert!(results.temporal_assertions.is_empty());
        assert_eq!(results.assertions.len(), 1);
        assert_eq!(results.assertions[0].id, "ready-once");
        assert_eq!(results.assertions[0].status, TraceAssertionStatus::Pass);
        assert_eq!(results.assertions[0].details.as_ref().unwrap()["actual"], 1);
    }

    #[test]
    fn count_assertion_fails_when_count_exceeds_max() {
        let mut results = results(
            vec![
                event(1, "log", "invalid_grant"),
                event(2, "log", "invalid_grant"),
            ],
            vec![TraceTemporalAssertionDefinition::Count {
                id: "no-invalid-grant".to_string(),
                events: vec!["log.invalid_grant".to_string()],
                min: None,
                max: Some(0),
                message: None,
            }],
        );

        assert!(apply_temporal_assertions(&mut results));
        assert_eq!(results.status, TraceStatus::Fail);
        assert_eq!(results.assertions[0].status, TraceAssertionStatus::Fail);
        assert_eq!(results.assertions[0].details.as_ref().unwrap()["actual"], 2);
    }

    #[test]
    fn forbidden_event_assertion_fails_on_match() {
        let mut results = results(
            vec![event(10, "desktop", "window.reopened")],
            vec![TraceTemporalAssertionDefinition::ForbiddenEvent {
                id: "no-window-reopen".to_string(),
                pattern: "desktop.window.reopened".to_string(),
                message: None,
            }],
        );

        assert!(apply_temporal_assertions(&mut results));
        assert_eq!(results.assertions[0].status, TraceAssertionStatus::Fail);
        assert_eq!(results.assertions[0].details.as_ref().unwrap()["actual"], 1);
    }

    #[test]
    fn max_concurrent_assertion_fails_when_overlap_exceeds_max() {
        let mut results = results(
            vec![
                event(0, "proc", "spawn"),
                event(5, "proc", "spawn"),
                event(10, "proc", "exit"),
                event(15, "proc", "exit"),
            ],
            vec![TraceTemporalAssertionDefinition::MaxConcurrent {
                id: "max-one-proc".to_string(),
                track: vec!["proc.spawn".to_string(), "proc.exit".to_string()],
                max: 1,
                message: None,
            }],
        );

        assert!(apply_temporal_assertions(&mut results));
        assert_eq!(results.assertions[0].status, TraceAssertionStatus::Fail);
        assert_eq!(
            results.assertions[0].details.as_ref().unwrap()["max_observed"],
            2
        );
    }
}

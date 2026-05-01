//! Generic trace span post-processing over `source.event` timeline keys.

use super::parsing::{
    TraceEvent, TraceResults, TraceSpanDefinition, TraceSpanResult, TraceSpanStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracePhaseMilestone {
    pub label: String,
    pub key: String,
}

pub(crate) fn parse_span_definition(raw: &str) -> Result<TraceSpanDefinition, String> {
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() != 3 {
        return Err("expected id:from:to".to_string());
    }
    let definition = TraceSpanDefinition {
        id: parts[0].trim().to_string(),
        from: parts[1].trim().to_string(),
        to: parts[2].trim().to_string(),
    };
    if definition.id.is_empty() || definition.from.is_empty() || definition.to.is_empty() {
        return Err("span id, from, and to must be non-empty".to_string());
    }
    Ok(definition)
}

pub(crate) fn parse_phase_milestone(raw: &str) -> Result<TracePhaseMilestone, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("phase milestone must be non-empty".to_string());
    }

    let (label, key) = match raw.split_once(':') {
        Some((label, key)) => (label.trim(), key.trim()),
        None => (raw, raw),
    };
    if label.is_empty() || key.is_empty() {
        return Err("expected [label:]source.event".to_string());
    }

    Ok(TracePhaseMilestone {
        label: label.to_string(),
        key: key.to_string(),
    })
}

pub(crate) fn phase_span_definitions(
    phases: &[TracePhaseMilestone],
) -> Result<Vec<TraceSpanDefinition>, String> {
    if phases.is_empty() {
        return Ok(Vec::new());
    }
    if phases.len() < 2 {
        return Err("at least two --phase milestones are required".to_string());
    }

    let mut definitions = phases
        .windows(2)
        .map(|pair| TraceSpanDefinition {
            id: format!(
                "phase.{}_to_{}",
                phase_id_part(&pair[0].label),
                phase_id_part(&pair[1].label)
            ),
            from: pair[0].key.clone(),
            to: pair[1].key.clone(),
        })
        .collect::<Vec<_>>();

    definitions.push(TraceSpanDefinition {
        id: "phase.total".to_string(),
        from: phases.first().expect("checked non-empty").key.clone(),
        to: phases.last().expect("checked non-empty").key.clone(),
    });

    Ok(definitions)
}

fn phase_id_part(label: &str) -> String {
    let mut out = String::new();
    let mut last_was_separator = false;
    for ch in label.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            out.push('_');
            last_was_separator = true;
        }
    }
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "phase".to_string()
    } else {
        trimmed
    }
}

pub(crate) fn apply_span_definitions(
    results: &mut TraceResults,
    cli_definitions: &[TraceSpanDefinition],
) {
    let definitions = merge_definitions(&results.span_definitions, cli_definitions);
    if definitions.is_empty() {
        return;
    }
    results.span_definitions = definitions.clone();
    let reporting_timeline = reporting_timeline(&results.timeline);
    results.span_results = summarize_spans(&reporting_timeline, &definitions);
}

fn reporting_timeline(timeline: &[TraceEvent]) -> Vec<TraceEvent> {
    let mut events = Vec::new();
    for event in timeline {
        push_reporting_event(&mut events, event.clone());
    }
    events.sort_by_key(|event| event.t_ms);
    events
}

fn push_reporting_event(events: &mut Vec<TraceEvent>, event: TraceEvent) {
    let nested = nested_detail_events(&event);
    events.push(event);
    for nested_event in nested {
        push_reporting_event(events, nested_event);
    }
}

fn nested_detail_events(event: &TraceEvent) -> Vec<TraceEvent> {
    let Some(serde_json::Value::Array(items)) = event.data.get("events") else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| nested_detail_event(event.t_ms, item))
        .collect()
}

fn nested_detail_event(parent_t_ms: u64, item: &serde_json::Value) -> Option<TraceEvent> {
    let source = item.get("source")?.as_str()?.to_string();
    let event = item.get("event")?.as_str()?.to_string();
    let relative_t_ms = json_millis(item.get("t")?)?;
    let data = item
        .get("data")
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(
                map.iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();

    Some(TraceEvent {
        t_ms: parent_t_ms.saturating_add(relative_t_ms),
        source,
        event,
        data,
    })
}

fn json_millis(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .map(|value| value.round() as u64)
        }),
        _ => None,
    }
}

pub(crate) fn summarize_spans(
    timeline: &[TraceEvent],
    definitions: &[TraceSpanDefinition],
) -> Vec<TraceSpanResult> {
    definitions
        .iter()
        .map(|definition| summarize_span(definition, timeline))
        .collect()
}

fn merge_definitions(
    runner_definitions: &[TraceSpanDefinition],
    cli_definitions: &[TraceSpanDefinition],
) -> Vec<TraceSpanDefinition> {
    let mut merged = Vec::new();
    for definition in runner_definitions.iter().chain(cli_definitions.iter()) {
        if let Some(position) = merged
            .iter()
            .position(|existing: &TraceSpanDefinition| existing.id == definition.id)
        {
            merged[position] = definition.clone();
        } else {
            merged.push(definition.clone());
        }
    }
    merged
}

fn event_matches_key(event: &TraceEvent, key: &str) -> bool {
    event.source.len() + 1 + event.event.len() == key.len()
        && key.starts_with(&event.source)
        && key.as_bytes().get(event.source.len()) == Some(&b'.')
        && key[event.source.len() + 1..] == event.event
}

fn first_event_time(timeline: &[TraceEvent], key: &str) -> Option<u64> {
    timeline
        .iter()
        .find(|event| event_matches_key(event, key))
        .map(|event| event.t_ms)
}

fn nearest_valid_pair(timeline: &[TraceEvent], from_key: &str, to_key: &str) -> Option<(u64, u64)> {
    let mut best: Option<(u64, u64)> = None;

    for from in timeline
        .iter()
        .filter(|event| event_matches_key(event, from_key))
    {
        for to in timeline
            .iter()
            .filter(|event| event_matches_key(event, to_key) && event.t_ms >= from.t_ms)
        {
            match best {
                Some((best_from, best_to))
                    if to.t_ms - from.t_ms >= best_to.saturating_sub(best_from) => {}
                _ => best = Some((from.t_ms, to.t_ms)),
            }
        }
    }

    best
}

fn out_of_order_span_message(
    definition: &TraceSpanDefinition,
    from_t_ms: u64,
    to_t_ms: u64,
) -> String {
    if definition.id.starts_with("phase.") {
        format!(
            "phase milestone `{}` occurred at {}ms before previous milestone `{}` at {}ms; phase chain is non-monotonic",
            definition.to, to_t_ms, definition.from, from_t_ms
        )
    } else {
        "span end occurred before span start".to_string()
    }
}

fn summarize_span(definition: &TraceSpanDefinition, timeline: &[TraceEvent]) -> TraceSpanResult {
    let from_t_ms = first_event_time(timeline, &definition.from);
    let to_t_ms = first_event_time(timeline, &definition.to);
    let mut missing = Vec::new();
    if from_t_ms.is_none() {
        missing.push(definition.from.clone());
    }
    if to_t_ms.is_none() {
        missing.push(definition.to.clone());
    }
    if !missing.is_empty() {
        return TraceSpanResult {
            id: definition.id.clone(),
            from: definition.from.clone(),
            to: definition.to.clone(),
            status: TraceSpanStatus::Skipped,
            duration_ms: None,
            from_t_ms,
            to_t_ms,
            missing,
            message: Some("span endpoint missing from timeline".to_string()),
        };
    }

    let Some((from_value, to_value)) =
        nearest_valid_pair(timeline, &definition.from, &definition.to)
    else {
        let message = match (from_t_ms, to_t_ms) {
            (Some(from_value), Some(to_value)) => {
                out_of_order_span_message(definition, from_value, to_value)
            }
            _ => "span end occurred before span start".to_string(),
        };
        return TraceSpanResult {
            id: definition.id.clone(),
            from: definition.from.clone(),
            to: definition.to.clone(),
            status: TraceSpanStatus::Skipped,
            duration_ms: None,
            from_t_ms,
            to_t_ms,
            missing: Vec::new(),
            message: Some(message),
        };
    };

    TraceSpanResult {
        id: definition.id.clone(),
        from: definition.from.clone(),
        to: definition.to.clone(),
        status: TraceSpanStatus::Ok,
        duration_ms: Some(to_value - from_value),
        from_t_ms: Some(from_value),
        to_t_ms: Some(to_value),
        missing: Vec::new(),
        message: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(t_ms: u64, source: &str, event: &str) -> TraceEvent {
        TraceEvent {
            t_ms,
            source: source.to_string(),
            event: event.to_string(),
            data: Default::default(),
        }
    }

    fn event_with_data(
        t_ms: u64,
        source: &str,
        event: &str,
        data: serde_json::Value,
    ) -> TraceEvent {
        let serde_json::Value::Object(data) = data else {
            panic!("test data must be a JSON object");
        };

        TraceEvent {
            t_ms,
            source: source.to_string(),
            event: event.to_string(),
            data: data.into_iter().collect(),
        }
    }

    #[test]
    fn test_parse_span_definition() {
        let definition = parse_span_definition("submit:ui.clicked:cli.started").unwrap();

        assert_eq!(definition.id, "submit");
        assert_eq!(definition.from, "ui.clicked");
        assert_eq!(definition.to, "cli.started");
    }

    #[test]
    fn test_parse_phase_milestone_with_label() {
        let phase = parse_phase_milestone("submit:ui.clicked").unwrap();

        assert_eq!(phase.label, "submit");
        assert_eq!(phase.key, "ui.clicked");
    }

    #[test]
    fn test_parse_phase_milestone_without_label() {
        let phase = parse_phase_milestone("ui.clicked").unwrap();

        assert_eq!(phase.label, "ui.clicked");
        assert_eq!(phase.key, "ui.clicked");
    }

    #[test]
    fn test_phase_span_definitions_include_adjacent_and_total() {
        let definitions = phase_span_definitions(&[
            TracePhaseMilestone {
                label: "submit".to_string(),
                key: "ui.clicked".to_string(),
            },
            TracePhaseMilestone {
                label: "cli start".to_string(),
                key: "cli.started".to_string(),
            },
            TracePhaseMilestone {
                label: "ready".to_string(),
                key: "server.ready".to_string(),
            },
        ])
        .unwrap();

        assert_eq!(definitions.len(), 3);
        assert_eq!(definitions[0].id, "phase.submit_to_cli_start");
        assert_eq!(definitions[0].from, "ui.clicked");
        assert_eq!(definitions[0].to, "cli.started");
        assert_eq!(definitions[1].id, "phase.cli_start_to_ready");
        assert_eq!(definitions[2].id, "phase.total");
        assert_eq!(definitions[2].from, "ui.clicked");
        assert_eq!(definitions[2].to, "server.ready");
    }

    #[test]
    fn phase_span_definitions_require_a_chain() {
        let error = phase_span_definitions(&[TracePhaseMilestone {
            label: "submit".to_string(),
            key: "ui.clicked".to_string(),
        }])
        .unwrap_err();

        assert_eq!(error, "at least two --phase milestones are required");
    }

    #[test]
    fn test_summarize_spans() {
        let results = summarize_spans(
            &[event(10, "ui", "clicked"), event(75, "cli", "started")],
            &[TraceSpanDefinition {
                id: "submit_to_cli".to_string(),
                from: "ui.clicked".to_string(),
                to: "cli.started".to_string(),
            }],
        );

        assert_eq!(results[0].status, TraceSpanStatus::Ok);
        assert_eq!(results[0].duration_ms, Some(65));
    }

    #[test]
    fn missing_span_endpoint_is_explicitly_skipped() {
        let results = summarize_spans(
            &[event(10, "ui", "clicked")],
            &[TraceSpanDefinition {
                id: "submit_to_cli".to_string(),
                from: "ui.clicked".to_string(),
                to: "cli.started".to_string(),
            }],
        );

        assert_eq!(results[0].status, TraceSpanStatus::Skipped);
        assert_eq!(results[0].duration_ms, None);
        assert_eq!(results[0].missing, vec!["cli.started"]);
    }

    #[test]
    fn repeated_events_resolve_to_nearest_valid_pair() {
        let results = summarize_spans(
            &[
                event(10, "renderer", "site_event_received"),
                event(50, "renderer", "site_event_received"),
                event(80, "renderer", "dom_status_running_seen"),
            ],
            &[TraceSpanDefinition {
                id: "site_running".to_string(),
                from: "renderer.site_event_received".to_string(),
                to: "renderer.dom_status_running_seen".to_string(),
            }],
        );

        assert_eq!(results[0].status, TraceSpanStatus::Ok);
        assert_eq!(results[0].from_t_ms, Some(50));
        assert_eq!(results[0].to_t_ms, Some(80));
        assert_eq!(results[0].duration_ms, Some(30));
    }

    #[test]
    fn repeated_events_skip_when_no_end_occurs_after_start() {
        let results = summarize_spans(
            &[
                event(10, "cli", "started"),
                event(50, "ui", "clicked"),
                event(75, "ui", "clicked"),
            ],
            &[TraceSpanDefinition {
                id: "submit_to_cli".to_string(),
                from: "ui.clicked".to_string(),
                to: "cli.started".to_string(),
            }],
        );

        assert_eq!(results[0].status, TraceSpanStatus::Skipped);
        assert_eq!(results[0].duration_ms, None);
        assert_eq!(results[0].from_t_ms, Some(50));
        assert_eq!(results[0].to_t_ms, Some(10));
        assert_eq!(
            results[0].message.as_deref(),
            Some("span end occurred before span start")
        );
    }

    #[test]
    fn out_of_order_phase_span_reports_non_monotonic_chain() {
        let results = summarize_spans(
            &[event(10, "runner", "ready"), event(50, "runner", "boot")],
            &[TraceSpanDefinition {
                id: "phase.boot_to_ready".to_string(),
                from: "runner.boot".to_string(),
                to: "runner.ready".to_string(),
            }],
        );

        assert_eq!(results[0].status, TraceSpanStatus::Skipped);
        assert_eq!(results[0].duration_ms, None);
        assert_eq!(results[0].from_t_ms, Some(50));
        assert_eq!(results[0].to_t_ms, Some(10));
        assert_eq!(
            results[0].message.as_deref(),
            Some(
                "phase milestone `runner.ready` occurred at 10ms before previous milestone `runner.boot` at 50ms; phase chain is non-monotonic"
            )
        );
    }

    #[test]
    fn test_apply_span_definitions() {
        let mut results = TraceResults {
            component_id: "studio".to_string(),
            scenario_id: "create-site".to_string(),
            status: crate::extension::trace::parsing::TraceStatus::Pass,
            summary: None,
            failure: None,
            rig: None,
            timeline: vec![event(10, "ui", "clicked"), event(30, "cli", "started")],
            span_definitions: Vec::new(),
            span_results: Vec::new(),
            assertions: Vec::new(),
            artifacts: Vec::new(),
        };

        apply_span_definitions(
            &mut results,
            &[TraceSpanDefinition {
                id: "submit_to_cli".to_string(),
                from: "ui.clicked".to_string(),
                to: "cli.started".to_string(),
            }],
        );

        assert_eq!(results.span_results[0].duration_ms, Some(20));
    }

    #[test]
    fn apply_span_definitions_uses_normalized_nested_detail_events() {
        let mut results = TraceResults {
            component_id: "generic".to_string(),
            scenario_id: "nested-detail-events".to_string(),
            status: crate::extension::trace::parsing::TraceStatus::Pass,
            summary: None,
            failure: None,
            rig: None,
            timeline: vec![event_with_data(
                100,
                "runner",
                "details",
                serde_json::json!({
                    "events": [
                        {
                            "source": "nested",
                            "event": "request.start",
                            "t": 7,
                            "data": {"url": "/example"}
                        },
                        {
                            "source": "nested",
                            "event": "request.ready",
                            "t": 42,
                            "data": {"status": 200}
                        }
                    ]
                }),
            )],
            span_definitions: Vec::new(),
            span_results: Vec::new(),
            assertions: Vec::new(),
            artifacts: Vec::new(),
        };

        apply_span_definitions(
            &mut results,
            &[TraceSpanDefinition {
                id: "nested_request".to_string(),
                from: "nested.request.start".to_string(),
                to: "nested.request.ready".to_string(),
            }],
        );

        assert_eq!(results.timeline.len(), 1);
        assert_eq!(results.span_results[0].status, TraceSpanStatus::Ok);
        assert_eq!(results.span_results[0].from_t_ms, Some(107));
        assert_eq!(results.span_results[0].to_t_ms, Some(142));
        assert_eq!(results.span_results[0].duration_ms, Some(35));
    }

    #[test]
    fn phase_span_definitions_can_select_nested_detail_events() {
        let mut results = TraceResults {
            component_id: "generic".to_string(),
            scenario_id: "nested-detail-events".to_string(),
            status: crate::extension::trace::parsing::TraceStatus::Pass,
            summary: None,
            failure: None,
            rig: None,
            timeline: vec![event_with_data(
                50,
                "runner",
                "details",
                serde_json::json!({
                    "events": [
                        {"source": "phase_source", "event": "first", "t": 1, "data": {}},
                        {"source": "phase_source", "event": "second", "t": 6, "data": {}},
                        {"source": "phase_source", "event": "third", "t": 20, "data": {}}
                    ]
                }),
            )],
            span_definitions: Vec::new(),
            span_results: Vec::new(),
            assertions: Vec::new(),
            artifacts: Vec::new(),
        };
        let phases = phase_span_definitions(&[
            TracePhaseMilestone {
                label: "first".to_string(),
                key: "phase_source.first".to_string(),
            },
            TracePhaseMilestone {
                label: "second".to_string(),
                key: "phase_source.second".to_string(),
            },
            TracePhaseMilestone {
                label: "third".to_string(),
                key: "phase_source.third".to_string(),
            },
        ])
        .unwrap();

        apply_span_definitions(&mut results, &phases);

        assert_eq!(results.span_results.len(), 3);
        assert_eq!(results.span_results[0].id, "phase.first_to_second");
        assert_eq!(results.span_results[0].duration_ms, Some(5));
        assert_eq!(results.span_results[1].id, "phase.second_to_third");
        assert_eq!(results.span_results[1].duration_ms, Some(14));
        assert_eq!(results.span_results[2].id, "phase.total");
        assert_eq!(results.span_results[2].duration_ms, Some(19));
    }
}

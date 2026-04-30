//! Generic trace span post-processing over `source.event` timeline keys.

use std::collections::HashMap;

use super::parsing::{
    TraceEvent, TraceResults, TraceSpanDefinition, TraceSpanResult, TraceSpanStatus,
};

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

pub(crate) fn apply_span_definitions(
    results: &mut TraceResults,
    cli_definitions: &[TraceSpanDefinition],
) {
    let definitions = merge_definitions(&results.span_definitions, cli_definitions);
    if definitions.is_empty() {
        return;
    }
    results.span_definitions = definitions.clone();
    results.span_results = summarize_spans(&results.timeline, &definitions);
}

pub(crate) fn summarize_spans(
    timeline: &[TraceEvent],
    definitions: &[TraceSpanDefinition],
) -> Vec<TraceSpanResult> {
    let index = first_event_index(timeline);
    definitions
        .iter()
        .map(|definition| summarize_span(definition, &index))
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

fn first_event_index(timeline: &[TraceEvent]) -> HashMap<String, u64> {
    let mut index = HashMap::new();
    for event in timeline {
        index.entry(timeline_key(event)).or_insert(event.t_ms);
    }
    index
}

fn timeline_key(event: &TraceEvent) -> String {
    format!("{}.{}", event.source, event.event)
}

fn summarize_span(
    definition: &TraceSpanDefinition,
    index: &HashMap<String, u64>,
) -> TraceSpanResult {
    let from_t_ms = index.get(&definition.from).copied();
    let to_t_ms = index.get(&definition.to).copied();
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

    let from_value = from_t_ms.expect("checked above");
    let to_value = to_t_ms.expect("checked above");
    if to_value < from_value {
        return TraceSpanResult {
            id: definition.id.clone(),
            from: definition.from.clone(),
            to: definition.to.clone(),
            status: TraceSpanStatus::Skipped,
            duration_ms: None,
            from_t_ms,
            to_t_ms,
            missing: Vec::new(),
            message: Some("span end occurred before span start".to_string()),
        };
    }

    TraceSpanResult {
        id: definition.id.clone(),
        from: definition.from.clone(),
        to: definition.to.clone(),
        status: TraceSpanStatus::Ok,
        duration_ms: Some(to_value - from_value),
        from_t_ms,
        to_t_ms,
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

    #[test]
    fn test_parse_span_definition() {
        let definition = parse_span_definition("submit:ui.clicked:cli.started").unwrap();

        assert_eq!(definition.id, "submit");
        assert_eq!(definition.from, "ui.clicked");
        assert_eq!(definition.to, "cli.started");
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
}

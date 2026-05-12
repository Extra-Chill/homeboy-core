//! Trace span post-processing over shared observation timeline primitives.

use super::parsing::{TraceEvent, TraceResults, TraceSpanDefinition, TraceSpanResult};
use crate::observation::timeline;

pub type TracePhaseMilestone = timeline::ObservationPhaseMilestone;

pub(crate) fn parse_span_definition(raw: &str) -> Result<TraceSpanDefinition, String> {
    timeline::parse_span_definition(raw)
}

pub(crate) fn parse_phase_milestone(raw: &str) -> Result<TracePhaseMilestone, String> {
    timeline::parse_phase_milestone(raw)
}

pub(crate) fn phase_span_definitions(
    phases: &[TracePhaseMilestone],
) -> Result<Vec<TraceSpanDefinition>, String> {
    timeline::phase_span_definitions(phases)
}

pub(crate) fn apply_span_definitions(
    results: &mut TraceResults,
    cli_definitions: &[TraceSpanDefinition],
) {
    let definitions = timeline::merge_span_definitions(&results.span_definitions, cli_definitions);
    if definitions.is_empty() {
        return;
    }
    results.span_definitions = definitions.clone();
    let reporting_timeline = reporting_timeline(&results.timeline);
    results.span_results = summarize_spans(&reporting_timeline, &definitions);
}

pub(crate) fn reporting_timeline(timeline: &[TraceEvent]) -> Vec<TraceEvent> {
    timeline::reporting_timeline(timeline)
}

pub(crate) fn summarize_spans(
    timeline: &[TraceEvent],
    definitions: &[TraceSpanDefinition],
) -> Vec<TraceSpanResult> {
    timeline::summarize_spans(timeline, definitions)
}

pub(crate) fn event_matches_key(event: &TraceEvent, key: &str) -> bool {
    timeline::event_matches_key(event, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::trace::parsing::TraceSpanStatus;

    fn event(t_ms: u64, source: &str, event: &str) -> TraceEvent {
        TraceEvent {
            t_ms,
            source: source.to_string(),
            event: event.to_string(),
            data: Default::default(),
        }
    }

    #[test]
    fn trace_wrappers_preserve_span_summary_contract() {
        let results = summarize_spans(
            &[event(10, "runner", "start"), event(35, "runner", "done")],
            &[TraceSpanDefinition {
                id: "total".to_string(),
                from: "runner.start".to_string(),
                to: "runner.done".to_string(),
            }],
        );

        assert_eq!(results[0].status, TraceSpanStatus::Ok);
        assert_eq!(results[0].duration_ms, Some(25));
    }
}

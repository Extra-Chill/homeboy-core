//! Generic bench metric comparison policy.

use serde::Serialize;

use super::parsing::{BenchMetricDirection, BenchMetricPolicy, BenchResults};

/// Per-metric delta vs baseline. Extensions can opt into comparing any
/// numeric metric by declaring a policy in the bench results JSON.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MetricDelta {
    pub name: String,
    pub baseline_value: f64,
    pub current_value: f64,
    /// Current minus baseline. Positive is not always bad — consult
    /// `direction` to know whether larger or smaller is better.
    pub delta: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_pct: Option<f64>,
    pub direction: BenchMetricDirection,
    pub regression: bool,
    pub improvement: bool,
}

impl MetricDelta {
    pub(crate) fn reason(&self, scenario_id: &str) -> String {
        let pct = self
            .delta_pct
            .map(|p| format!(" ({:+.1}%)", p))
            .unwrap_or_default();
        format!(
            "{}: {} {:.2} → {:.2}{}",
            scenario_id, self.name, self.baseline_value, self.current_value, pct
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedMetricPolicy {
    name: String,
    policy: BenchMetricPolicy,
    regression_threshold_absolute: f64,
    zero_baseline_is_neutral: bool,
}

impl ResolvedMetricPolicy {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn compare(&self, baseline_value: f64, current_value: f64) -> MetricDelta {
        let delta = current_value - baseline_value;
        let bad_delta = match self.policy.direction {
            BenchMetricDirection::LowerIsBetter => delta,
            BenchMetricDirection::HigherIsBetter => -delta,
        };
        let delta_pct = if baseline_value != 0.0 {
            Some((delta / baseline_value) * 100.0)
        } else {
            None
        };
        let bad_delta_pct = if baseline_value != 0.0 {
            Some((bad_delta / baseline_value.abs()) * 100.0)
        } else {
            None
        };

        let worse = bad_delta > 0.0;
        let regression = if self.zero_baseline_is_neutral && baseline_value == 0.0 {
            false
        } else {
            let exceeds_absolute = bad_delta > self.regression_threshold_absolute;
            let exceeds_percent = match self.policy.regression_threshold_percent {
                Some(threshold) => bad_delta_pct.map(|pct| pct > threshold).unwrap_or(worse),
                None => true,
            };
            worse && exceeds_absolute && exceeds_percent
        };

        MetricDelta {
            name: self.name.clone(),
            baseline_value,
            current_value,
            delta,
            delta_pct,
            direction: self.policy.direction,
            regression,
            improvement: bad_delta < 0.0,
        }
    }

    fn custom(name: &str, policy: &BenchMetricPolicy) -> Self {
        Self {
            name: name.to_string(),
            policy: policy.clone(),
            regression_threshold_absolute: policy.regression_threshold_absolute.unwrap_or(0.0),
            zero_baseline_is_neutral: false,
        }
    }

    fn legacy_p95(default_threshold_percent: f64) -> Self {
        Self {
            name: "p95_ms".to_string(),
            policy: BenchMetricPolicy {
                direction: BenchMetricDirection::LowerIsBetter,
                regression_threshold_percent: Some(default_threshold_percent),
                regression_threshold_absolute: Some(0.0),
            },
            regression_threshold_absolute: 0.0,
            zero_baseline_is_neutral: true,
        }
    }
}

pub(crate) fn resolve_metric_policies(
    current: &BenchResults,
    default_threshold_percent: f64,
) -> Vec<ResolvedMetricPolicy> {
    if current.metric_policies.is_empty() {
        let has_p95 = current
            .scenarios
            .iter()
            .any(|scenario| scenario.metrics.get("p95_ms").is_some());
        if !has_p95 {
            return Vec::new();
        }
        return vec![ResolvedMetricPolicy::legacy_p95(default_threshold_percent)];
    }

    current
        .metric_policies
        .iter()
        .map(|(name, policy)| ResolvedMetricPolicy::custom(name, policy))
        .collect()
}

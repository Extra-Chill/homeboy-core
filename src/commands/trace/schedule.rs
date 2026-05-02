use clap::ValueEnum;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum TraceSchedule {
    Grouped,
    Interleaved,
}

impl TraceSchedule {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Grouped => "grouped",
            Self::Interleaved => "interleaved",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
pub enum TraceVariantMatrixMode {
    #[default]
    None,
    Single,
    Cumulative,
}

impl TraceVariantMatrixMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Single => "single",
            Self::Cumulative => "cumulative",
        }
    }
}

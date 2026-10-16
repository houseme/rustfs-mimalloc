/// Failure to configure or use a profiling session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// A configuration field is invalid or unsupported on this platform.
    InvalidConfig(&'static str),
    /// The sampling allocator has not observed any allocation in this process.
    AllocatorNotObserved,
    /// Another profiling session already owns the process recorder.
    AlreadyActive,
    /// A stopped session cannot produce additional reports.
    Inactive,
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidConfig(message) => message,
            Self::AllocatorNotObserved => {
                "mimalloc: SamplingMiMalloc has not observed allocations; install it as #[global_allocator] before starting profiling"
            }
            Self::AlreadyActive => "mimalloc: another profiling session is already active",
            Self::Inactive => "mimalloc: profiling session has stopped",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for Error {}

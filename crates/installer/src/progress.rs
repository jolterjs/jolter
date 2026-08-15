#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressAction {
    Select,
    Resolve,
    Reuse,
    Connect,
    Download,
    Verify,
    Extract,
    Publish,
    Activate,
    Remove,
    Clean,
    Diagnose,
    Configure,
    Shims,
}

impl ProgressAction {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Resolve => "resolve",
            Self::Reuse => "reuse",
            Self::Connect => "connect",
            Self::Download => "fetch",
            Self::Verify => "verify",
            Self::Extract => "unpack",
            Self::Publish => "install",
            Self::Activate => "activate",
            Self::Remove => "remove",
            Self::Clean => "clean",
            Self::Diagnose => "doctor",
            Self::Configure => "config",
            Self::Shims => "shims",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressEvent<'a> {
    Stage {
        action: ProgressAction,
        target: &'a str,
    },
    DownloadStarted {
        name: &'a str,
        total: Option<u64>,
    },
    DownloadAdvanced {
        name: &'a str,
        downloaded: u64,
        total: Option<u64>,
    },
    DownloadFinished {
        name: &'a str,
        downloaded: u64,
        total: Option<u64>,
    },
    CacheHit {
        name: &'a str,
    },
}

pub trait ProgressReporter: Send + Sync {
    fn report(&self, event: ProgressEvent<'_>);
}

#[derive(Debug, Default)]
pub struct NoProgressReporter;

impl ProgressReporter for NoProgressReporter {
    fn report(&self, _event: ProgressEvent<'_>) {}
}

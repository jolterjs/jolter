#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPreference {
    Auto,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPreference {
    Auto,
    Never,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailLevel {
    Quiet,
    Normal,
    Verbose,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Human,
    Machine,
}

#[derive(Debug, Clone, Copy)]
pub struct OutputOptions {
    pub progress: ProgressPreference,
    pub color: ColorPreference,
    pub detail: DetailLevel,
    pub kind: OutputKind,
}

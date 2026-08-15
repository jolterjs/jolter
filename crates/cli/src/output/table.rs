#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRow {
    pub(crate) marker: char,
    pub(crate) label: String,
    pub(crate) status: String,
    pub(crate) detail: String,
}

impl TableRow {
    #[must_use]
    pub fn new(
        marker: char,
        label: impl Into<String>,
        status: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            marker,
            label: label.into(),
            status: status.into(),
            detail: detail.into(),
        }
    }
}

pub fn format_table_rows(rows: &[TableRow]) -> Vec<String> {
    let label_width = rows
        .iter()
        .map(|row| row.label.chars().count())
        .max()
        .unwrap_or_default();
    let status_width = rows
        .iter()
        .map(|row| row.status.chars().count())
        .max()
        .unwrap_or_default();

    rows.iter()
        .map(|row| {
            format!(
                "{} {:label_width$}  {:status_width$}  {}",
                row.marker, row.label, row.status, row.detail
            )
        })
        .collect()
}

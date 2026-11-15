//! Export helpers: CSV reports of gated population counts and percentages,
//! plus statistics tables derived from population profiles.

use std::io::Write;
use std::path::Path;

use crate::statistics::PopulationProfile;

/// One row of a population report.
#[derive(Debug, Clone, PartialEq)]
pub struct PopulationRow {
    /// Sample (file) name.
    pub sample: String,
    /// Population / gate name.
    pub population: String,
    /// Name of the parent population, if any.
    pub parent: Option<String>,
    /// Number of events in this population.
    pub count: usize,
    /// Total number of events in the sample.
    pub total: usize,
    /// Number of events in the parent population, if any.
    pub parent_count: Option<usize>,
    /// Optional gate description (e.g. "rect[0,1] x∈[0,1000]").
    pub gate_summary: Option<String>,
}

impl PopulationRow {
    /// Percentage of the whole sample.
    pub fn percent_of_total(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.count as f64 / self.total as f64 * 100.0
        }
    }

    /// Percentage of the parent population, if known.
    pub fn percent_of_parent(&self) -> Option<f64> {
        self.parent_count.map(|pc| {
            if pc == 0 {
                0.0
            } else {
                self.count as f64 / pc as f64 * 100.0
            }
        })
    }
}

/// Escapes a field for CSV if it contains a comma, quote, or newline.
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Renders population rows as a CSV document.
pub fn populations_csv(rows: &[PopulationRow]) -> String {
    let mut out = String::new();
    out.push_str(
        "Sample,Population,Parent,Count,Total,PercentOfTotal,PercentOfParent,GateSummary\n",
    );
    for row in rows {
        let parent = row.parent.clone().unwrap_or_default();
        let pct_parent = match row.percent_of_parent() {
            Some(v) => format!("{v:.4}"),
            None => String::new(),
        };
        let gate = row.gate_summary.clone().unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{:.4},{},{}\n",
            csv_field(&row.sample),
            csv_field(&row.population),
            csv_field(&parent),
            row.count,
            row.total,
            row.percent_of_total(),
            pct_parent,
            csv_field(&gate),
        ));
    }
    out
}

/// Writes population rows to a `.csv` file.
pub fn write_populations_csv(path: &Path, rows: &[PopulationRow]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(populations_csv(rows).as_bytes())
}

/// One row of a statistics table: sample x population x statistic value.
#[derive(Debug, Clone, PartialEq)]
pub struct StatisticRow {
    /// Sample (file) name the statistic was computed from.
    pub sample: String,
    /// Population / gate name.
    pub population: String,
    /// Human-readable statistic label (e.g. "mean", "p95").
    pub statistic: String,
    /// Computed value.
    pub value: f64,
}

/// Flatten population profiles into statistics-table rows for one sample.
pub fn statistics_rows(sample: &str, profiles: &[PopulationProfile]) -> Vec<StatisticRow> {
    let mut rows = Vec::new();
    for profile in profiles {
        for stat in &profile.stats {
            rows.push(StatisticRow {
                sample: sample.to_string(),
                population: profile.name.clone(),
                statistic: crate::statistics::statistic_label(&stat.statistic),
                value: stat.value,
            });
        }
    }
    rows
}

/// Renders statistics rows as a CSV document: `Sample,Population,Statistic,Value`.
pub fn statistics_csv(rows: &[StatisticRow]) -> String {
    let mut out = String::from("Sample,Population,Statistic,Value\n");
    for row in rows {
        out.push_str(&format!(
            "{},{},{},{:.6}\n",
            csv_field(&row.sample),
            csv_field(&row.population),
            csv_field(&row.statistic),
            row.value
        ));
    }
    out
}

/// Writes statistics rows to a `.csv` file.
pub fn write_statistics_csv(path: &Path, rows: &[StatisticRow]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(statistics_csv(rows).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_contains_header_and_values() {
        let rows = vec![PopulationRow {
            sample: "sample.fcs".to_string(),
            population: "Live".to_string(),
            parent: None,
            count: 25,
            total: 100,
            parent_count: None,
            gate_summary: Some("rect x[0,10]".to_string()),
        }];
        let csv = populations_csv(&rows);
        assert!(csv.starts_with("Sample,Population,Parent"));
        assert!(csv.contains("sample.fcs,Live,,25,100,25.0000,,"));
    }

    #[test]
    fn percent_of_parent() {
        let row = PopulationRow {
            sample: "s".into(),
            population: "child".into(),
            parent: Some("parent".into()),
            count: 5,
            total: 100,
            parent_count: Some(20),
            gate_summary: None,
        };
        assert!((row.percent_of_total() - 5.0).abs() < 1e-9);
        assert!((row.percent_of_parent().unwrap() - 25.0).abs() < 1e-9);
    }

    #[test]
    fn csv_escapes_commas() {
        let rows = vec![PopulationRow {
            sample: "a,b".into(),
            population: "p".into(),
            parent: None,
            count: 1,
            total: 1,
            parent_count: None,
            gate_summary: None,
        }];
        assert!(populations_csv(&rows).contains("\"a,b\""));
    }

    #[test]
    fn statistics_csv_flattens_profiles_to_rows() {
        let profile = PopulationProfile {
            name: "Live".to_string(),
            count: 25,
            parent_count: None,
            total_count: Some(100),
            stats: vec![
                crate::statistics::PopulationStat {
                    parameter: "FSC".to_string(),
                    statistic: crate::statistics::Statistic::Mean,
                    value: 3.5,
                },
                crate::statistics::PopulationStat {
                    parameter: "CD4".to_string(),
                    statistic: crate::statistics::Statistic::Median,
                    value: 7.25,
                },
            ],
        };
        let rows = statistics_rows("sample.fcs", &[profile]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].statistic, "mean");
        assert_eq!(rows[1].statistic, "median");
        let csv = statistics_csv(&rows);
        assert!(csv.starts_with("Sample,Population,Statistic,Value"));
        assert!(csv.contains("sample.fcs,Live,mean,3.500000"));
        assert!(csv.contains("sample.fcs,Live,median,7.250000"));
    }
}

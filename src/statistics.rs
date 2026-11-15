use serde::{Deserialize, Serialize};

use crate::gating::RangeGate;

/// A single summary statistic.
///
/// Parameterised variants carry their parameter inline so a statistic and its
/// meaning survive project serialization. `Percentile(50)` is therefore
/// distinguishable from `Median`, even though their values usually agree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Statistic {
    Count,
    PercentOfParent,
    PercentOfTotal,
    /// Frequency of this population relative to a named reference population
    /// (an ancestor, the grand total, or any user-picked denominator).
    FreqOf {
        /// Name of the reference population, e.g. `"Parent"`, `"Grandparent"`,
        /// or `"Total"`.
        denominator: String,
    },
    Mean,
    Median,
    StdDev,
    Cv,      // coefficient of variation (std / mean)
    GeoMean, // geometric mean of positive values
    Min,
    Max,
    Mode,
    Percentile(f64),
    Percent5,
    Percent95,
    Percent10,
    Percent90,
    RobustSd,
    RobustCv,
    /// Fraction of events at or below the supplied value.
    Cdf(f64),
}

/// Computed statistics for one population across one parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PopulationStat {
    pub parameter: String,
    pub statistic: Statistic,
    pub value: f64,
}

/// A full statistical profile of a population: one `PopulationStat` per
/// (parameter, statistic) pair.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PopulationProfile {
    pub name: String,
    pub count: usize,
    pub parent_count: Option<usize>,
    pub total_count: Option<usize>,
    pub stats: Vec<PopulationStat>,
}

impl PopulationProfile {
    pub fn percent_of_parent(&self) -> Option<f64> {
        self.parent_count.map(|pc| {
            if pc == 0 {
                0.0
            } else {
                (self.count as f64 / pc as f64) * 100.0
            }
        })
    }

    pub fn percent_of_total(&self) -> Option<f64> {
        self.total_count.map(|tc| {
            if tc == 0 {
                0.0
            } else {
                (self.count as f64 / tc as f64) * 100.0
            }
        })
    }

    /// Percent of this population relative to an arbitrary denominator count
    /// (an ancestor, a grandparent, the grand total, or any other reference).
    ///
    /// Returns `None` when the denominator is unknown. A zero denominator is
    /// reported as zero percent so callers never divide by zero.
    pub fn percent_of(&self, denominator_count: usize) -> Option<f64> {
        if denominator_count == 0 {
            return Some(0.0);
        }
        Some((self.count as f64 / denominator_count as f64) * 100.0)
    }
}

/// Specification for one statistic inside a [`StatisticSet`].
///
/// `parameter` selects a named parameter, or `None` to apply the statistic to
/// every parameter in the profile. Population-level statistics (`Count`,
/// `PercentOfParent`, `PercentOfTotal`, `FreqOf`) match on the statistic kind
/// alone, so `parameter: None` selects them directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatSpec {
    pub parameter: Option<String>,
    pub statistic: Statistic,
    /// Expected range for this statistic (None = no expectation configured).
    #[serde(default)]
    pub expected: Option<ExpectedRange>,
    /// Heat-map range for this statistic (None = no heat range configured).
    #[serde(default)]
    pub heat: Option<HeatRange>,
}

/// Expected range for one statistic: values outside `[min, max]` are flagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectedRange {
    pub min: f64,
    pub max: f64,
    pub warn_only: bool,
}

impl Default for ExpectedRange {
    fn default() -> Self {
        Self {
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            warn_only: false,
        }
    }
}

/// Heat-map range for one statistic: normalises a value into `[min, max]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeatRange {
    pub min: f64,
    pub max: f64,
    pub palette: String,
}

impl Default for HeatRange {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            palette: "viridis".to_string(),
        }
    }
}

/// A named, reusable collection of statistics computed together for selected
/// populations and parameters.
///
/// FlowJo statistic sets let an analysis define one list of statistics and
/// apply it consistently across populations, parameters, tables and reports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatisticSet {
    pub name: String,
    pub stats: Vec<StatSpec>,
}

impl StatSpec {
    /// Expected range configured for this statistic, if any.
    pub fn expected_range(&self) -> Option<ExpectedRange> {
        self.expected.clone()
    }

    /// Heat-map range configured for this statistic, if any.
    pub fn heat_range(&self) -> Option<HeatRange> {
        self.heat.clone()
    }
}

impl StatisticSet {
    pub fn new(name: &str, stats: Vec<StatSpec>) -> Self {
        Self {
            name: name.to_string(),
            stats,
        }
    }

    fn spec(statistic: Statistic) -> StatSpec {
        StatSpec {
            parameter: None,
            statistic,
            expected: None,
            heat: None,
        }
    }

    /// Expected range configured for the given parameter + statistic, if any
    /// statistic in this set defines one.
    pub fn expected_range_for(
        &self,
        parameter: &str,
        statistic: &Statistic,
    ) -> Option<ExpectedRange> {
        self.stats
            .iter()
            .find(|spec| {
                spec.parameter.as_deref() == Some(parameter) && spec.statistic == *statistic
            })
            .and_then(StatSpec::expected_range)
    }

    /// Heat-map range configured for the given parameter + statistic, if any
    /// statistic in this set defines one.
    pub fn heat_range_for(&self, parameter: &str, statistic: &Statistic) -> Option<HeatRange> {
        self.stats
            .iter()
            .find(|spec| {
                spec.parameter.as_deref() == Some(parameter) && spec.statistic == *statistic
            })
            .and_then(StatSpec::heat_range)
    }

    /// Whether a computed value sits inside the expected range.
    pub fn in_range(&self, parameter: &str, statistic: &Statistic, value: f64) -> bool {
        self.expected_range_for(parameter, statistic)
            .map(|range| value >= range.min && value <= range.max)
            .unwrap_or(true)
    }

    /// Normalised heat position of a value within its heat range, clamped to
    /// `[0, 1]`.
    pub fn heat_position(&self, parameter: &str, statistic: &Statistic, value: f64) -> Option<f64> {
        self.heat_range_for(parameter, statistic).map(|heat| {
            if heat.max <= heat.min {
                return 0.0;
            }
            ((value - heat.min) / (heat.max - heat.min)).clamp(0.0, 1.0)
        })
    }

    /// Basic preset: count and frequencies plus the core location/spread
    /// statistics for every parameter.
    pub fn basic() -> Self {
        Self::new(
            "Basic",
            vec![
                Self::spec(Statistic::Count),
                Self::spec(Statistic::PercentOfParent),
                Self::spec(Statistic::PercentOfTotal),
                Self::spec(Statistic::Mean),
                Self::spec(Statistic::Median),
                Self::spec(Statistic::StdDev),
                Self::spec(Statistic::Cv),
                Self::spec(Statistic::GeoMean),
                Self::spec(Statistic::Min),
                Self::spec(Statistic::Max),
            ],
        )
    }

    /// Robust preset: basic statistics plus mode, 5th/95th percentiles and the
    /// median-based robust SD/CV.
    pub fn robust() -> Self {
        let mut set = Self::basic();
        set.name = "Robust".to_string();
        set.stats.extend([
            Self::spec(Statistic::Mode),
            Self::spec(Statistic::Percent5),
            Self::spec(Statistic::Percent95),
            Self::spec(Statistic::RobustSd),
            Self::spec(Statistic::RobustCv),
        ]);
        set
    }

    /// Full preset: every statistic produced by [`compute_profile`] except the
    /// value-specific `Cdf(at)`, which depends on the profile's median.
    pub fn full() -> Self {
        let mut set = Self::basic();
        set.name = "Full".to_string();
        set.stats.extend([
            Self::spec(Statistic::Mode),
            Self::spec(Statistic::Percent5),
            Self::spec(Statistic::Percent10),
            Self::spec(Statistic::Percent90),
            Self::spec(Statistic::Percent95),
            Self::spec(Statistic::RobustSd),
            Self::spec(Statistic::RobustCv),
        ]);
        set
    }

    /// The presets exposed by the UI.
    pub fn presets() -> [StatisticSet; 3] {
        [Self::basic(), Self::robust(), Self::full()]
    }

    /// Apply the set to a computed profile.
    ///
    /// Entries are returned in set order and de-duplicated by
    /// `(parameter, statistic)`.
    pub fn apply(&self, profile: &PopulationProfile) -> Vec<PopulationStat> {
        let mut out: Vec<PopulationStat> = Vec::new();
        for spec in &self.stats {
            for stat in &profile.stats {
                if stat.statistic != spec.statistic {
                    continue;
                }
                if let Some(parameter) = &spec.parameter
                    && &stat.parameter != parameter
                {
                    continue;
                }
                let duplicate = out
                    .iter()
                    .any(|s| s.parameter == stat.parameter && s.statistic == stat.statistic);
                if !duplicate {
                    out.push(stat.clone());
                }
            }
        }
        out
    }

    /// Parameter names referenced by the set, expanding wildcard (`None`)
    /// entries to every analysis parameter present in the profile. Pseudo
    /// parameters (`%Parent`, `%Total`, `%<denominator>`) are excluded.
    pub fn parameter_names(&self, profile: &PopulationProfile) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for spec in &self.stats {
            match &spec.parameter {
                Some(p) => {
                    if !names.contains(p) {
                        names.push(p.clone());
                    }
                }
                None => {
                    for stat in &profile.stats {
                        if stat.parameter.starts_with('%') {
                            continue;
                        }
                        if !names.contains(&stat.parameter) {
                            names.push(stat.parameter.clone());
                        }
                    }
                }
            }
        }
        names
    }
}

/// Linear-interpolated percentile (the R-7 / Excel `PERCENTILE.INC` method).
///
/// `p` is clamped to `[0, 100]`. The input is copied and sorted, so callers do
/// not need to pre-sort. Empty input returns `NaN`.
pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    percentile_sorted(&sorted, p)
}

/// Percentile of an already-sorted finite slice.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        sorted[lower]
    } else {
        let f = rank - lower as f64;
        sorted[lower] * (1.0 - f) + sorted[upper] * f
    }
}

/// Median of an already-sorted finite slice.
fn median_sorted(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    if sorted.len() % 2 == 1 {
        sorted[sorted.len() / 2]
    } else {
        let mid = sorted.len() / 2;
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

/// Median absolute deviation, scaled by 1.4826 to estimate a normal SD.
fn robust_sd_sorted(median: f64, sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let mut deviations: Vec<f64> = sorted.iter().map(|v| (v - median).abs()).collect();
    deviations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mad = median_sorted(&deviations);
    1.4826 * mad
}

/// Mode of a finite-value slice. Prefers the most frequent value; falls back to
/// the centre of the widest-fraction bin for continuous data with all-unique
/// values.
pub fn mode(values: &[f64]) -> f64 {
    let mut finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return f64::NAN;
    }
    finite.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if finite.len() == 1 {
        return finite[0];
    }
    // Most frequent value (handles discrete / quantized channels and constant
    // populations).
    let mut best_value = finite[0];
    let mut best_count = 1usize;
    let mut count: usize = 1;
    for i in 1..finite.len() {
        if finite[i] == finite[i - 1] {
            count += 1;
        } else {
            if count > best_count {
                best_count = count;
                best_value = finite[i - 1];
            }
            count = 1;
        }
    }
    if count > best_count {
        best_value = finite[finite.len() - 1];
    }
    if best_count > 1 {
        return best_value;
    }
    // All values unique: histogram mode via equal-width bins.
    let min = finite[0];
    let max = finite[finite.len() - 1];
    if (max - min).abs() < f64::EPSILON {
        return min;
    }
    let bin_count = (((max - min) / ((max - min) / 20.0)).ceil() as usize).clamp(1, 4096);
    let width = (max - min) / bin_count as f64;
    let mut counts = vec![0usize; bin_count];
    for &v in &finite {
        let idx = ((v - min) / width).floor() as usize;
        let idx = idx.min(bin_count - 1);
        counts[idx] += 1;
    }
    let best = counts
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)
        .map(|(i, _)| i)
        .unwrap_or(0);
    min + (best as f64 + 0.5) * width
}

/// Empirical CDF at `value`: fraction of finite events `<= value`.
pub fn cdf_at(values: &[f64], value: f64) -> f64 {
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.is_empty() {
        return f64::NAN;
    }
    let count = finite.iter().filter(|v| **v <= value).count();
    count as f64 / finite.len() as f64
}

/// Compute statistics for a subset of events (a population) on a dataset.
///
/// `data` has shape `(events, parameters)`. `indices` selects the events
/// belonging to the population. `parameter_names` labels each column.
///
/// This emits the built-in percent statistics (`%Parent`, `%Total`) together
/// with the full set of per-parameter statistics (mean, median, mode, SD, CV,
/// percentiles, robust statistics, CDF, ...).
pub fn compute_profile(
    name: &str,
    data: &ndarray::Array2<f32>,
    indices: &[usize],
    parameter_names: &[String],
    parent_count: Option<usize>,
    total_count: Option<usize>,
) -> PopulationProfile {
    compute_profile_with_denominators(
        name,
        data,
        indices,
        parameter_names,
        parent_count,
        total_count,
        &[
            ("Parent".to_string(), parent_count),
            ("Total".to_string(), total_count),
        ],
    )
}

/// Compute a population profile with arbitrary "Freq of" statistics.
///
/// In addition to the built-in parent/total percentages, this accepts a list of
/// frequency denominators: `(label, Some(count))` pairs naming reference
/// populations such as an ancestor, a grandparent, or the grand total. Labels
/// other than `"Parent"` / `"Total"` produce [`Statistic::FreqOf`] entries, so a
/// child population can report its frequency against any ancestor population.
///
/// `parent_count` / `total_count` still describe the immediate parent and the
/// whole sample and are emitted as before; pass the full ancestry in
/// `frequencies` for nested references.
pub fn compute_profile_with_denominators(
    name: &str,
    data: &ndarray::Array2<f32>,
    indices: &[usize],
    parameter_names: &[String],
    parent_count: Option<usize>,
    total_count: Option<usize>,
    frequencies: &[(String, Option<usize>)],
) -> PopulationProfile {
    let (_, n_params) = data.dim();
    let mut stats: Vec<PopulationStat> = Vec::new();

    let count = indices.len();
    let profile_count = count;

    let push_frequency =
        |stats: &mut Vec<PopulationStat>, label: &str, denominator_count: Option<usize>| {
            if let Some(d) = denominator_count {
                let value = if d == 0 {
                    0.0
                } else {
                    (count as f64 / d as f64) * 100.0
                };
                let statistic = if label == "Parent" {
                    Statistic::PercentOfParent
                } else if label == "Total" {
                    Statistic::PercentOfTotal
                } else {
                    Statistic::FreqOf {
                        denominator: label.to_string(),
                    }
                };
                stats.push(PopulationStat {
                    parameter: format!("%{label}"),
                    statistic,
                    value,
                });
            }
        };

    push_frequency(&mut stats, "Parent", parent_count);
    push_frequency(&mut stats, "Total", total_count);
    for (label, denominator_count) in frequencies {
        push_frequency(&mut stats, label, *denominator_count);
    }

    for p in 0..n_params {
        let parameter_name = parameter_names
            .get(p)
            .cloned()
            .unwrap_or_else(|| format!("P{p}"));
        // Collect the population's values for this parameter.
        let mut values: Vec<f64> = indices
            .iter()
            .map(|&i| data[[i, p]] as f64)
            .filter(|v| v.is_finite())
            .collect();
        if values.is_empty() {
            continue;
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let sum: f64 = values.iter().sum();
        let mean = sum / values.len() as f64;
        let variance: f64 =
            values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();
        let cv = if mean != 0.0 {
            std_dev / mean.abs()
        } else {
            f64::NAN
        };

        let median = median_sorted(&values);

        // Geometric mean of strictly positive values.
        let geo_mean: f64 = if values.iter().all(|v| *v > 0.0) {
            values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64
        } else {
            f64::NAN
        }
        .exp();

        let min = values[0];
        let max = values[values.len() - 1];
        let p5 = percentile_sorted(&values, 5.0);
        let p10 = percentile_sorted(&values, 10.0);
        let p90 = percentile_sorted(&values, 90.0);
        let p95 = percentile_sorted(&values, 95.0);
        let median_mode = mode(&values);
        let robust_sd = robust_sd_sorted(median, &values);
        let robust_cv = if median != 0.0 {
            robust_sd / median.abs()
        } else {
            f64::NAN
        };
        let cdf_median = cdf_at(&values, median);

        let push = |stats: &mut Vec<PopulationStat>, statistic: Statistic, value: f64| {
            stats.push(PopulationStat {
                parameter: parameter_name.clone(),
                statistic,
                value,
            });
        };
        push(&mut stats, Statistic::Count, count as f64);
        push(&mut stats, Statistic::Mean, mean);
        push(&mut stats, Statistic::Median, median);
        push(&mut stats, Statistic::Mode, median_mode);
        push(&mut stats, Statistic::StdDev, std_dev);
        push(&mut stats, Statistic::RobustSd, robust_sd);
        push(&mut stats, Statistic::Cv, cv);
        push(&mut stats, Statistic::RobustCv, robust_cv);
        push(&mut stats, Statistic::GeoMean, geo_mean);
        push(&mut stats, Statistic::Min, min);
        push(&mut stats, Statistic::Percent5, p5);
        push(&mut stats, Statistic::Percent10, p10);
        push(&mut stats, Statistic::Percent90, p90);
        push(&mut stats, Statistic::Percent95, p95);
        push(&mut stats, Statistic::Max, max);
        push(&mut stats, Statistic::Cdf(median), cdf_median);
    }

    PopulationProfile {
        name: name.to_string(),
        count: profile_count,
        parent_count,
        total_count,
        stats,
    }
}

/// Human-readable label for a statistic, used in tables, panels and reports.
pub fn statistic_label(statistic: &Statistic) -> String {
    match statistic {
        Statistic::Count => "count".to_string(),
        Statistic::PercentOfParent => "%parent".to_string(),
        Statistic::PercentOfTotal => "%total".to_string(),
        Statistic::FreqOf { denominator } => format!("freq of {denominator}"),
        Statistic::Mean => "mean".to_string(),
        Statistic::Median => "median".to_string(),
        Statistic::StdDev => "std".to_string(),
        Statistic::Cv => "CV".to_string(),
        Statistic::GeoMean => "geoMean".to_string(),
        Statistic::Min => "min".to_string(),
        Statistic::Max => "max".to_string(),
        Statistic::Mode => "mode".to_string(),
        Statistic::Percentile(p) => format!("p{p}"),
        Statistic::Percent5 => "p5".to_string(),
        Statistic::Percent95 => "p95".to_string(),
        Statistic::Percent10 => "p10".to_string(),
        Statistic::Percent90 => "p90".to_string(),
        Statistic::RobustSd => "robustSD".to_string(),
        Statistic::RobustCv => "robustCV".to_string(),
        Statistic::Cdf(at) => format!("CDF({at:.4})"),
    }
}

/// Pretty-print a population profile to stdout.
pub fn print_profile(profile: &PopulationProfile) {
    println!("Population: {}", profile.name);
    println!("  count = {}", profile.count);
    if let Some(p) = profile.percent_of_parent() {
        println!("  % of parent = {:.2}%", p);
    }
    if let Some(p) = profile.percent_of_total() {
        println!("  % of total = {:.2}%", p);
    }
    for stat in &profile.stats {
        if let Statistic::FreqOf { .. } = &stat.statistic {
            let Statistic::FreqOf { denominator } = &stat.statistic else {
                continue;
            };
            println!("  freq of {denominator} = {:.2}%", stat.value);
            continue;
        }
        match &stat.statistic {
            Statistic::Count | Statistic::PercentOfParent | Statistic::PercentOfTotal => {}
            _ => {
                let label = statistic_label(&stat.statistic);
                if stat.value.is_nan() {
                    println!("  {} {} = NaN", stat.parameter, label);
                } else {
                    println!("  {} {} = {:.4}", stat.parameter, label, stat.value);
                }
            }
        }
    }
}

/// Two-sample Kolmogorov–Smirnov statistic: the maximum absolute difference
/// between the empirical CDFs of `a` and `b` (A4).
pub fn ks_statistic(a: &[f64], b: &[f64]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let n_a = a.len() as f64;
    let n_b = b.len() as f64;
    let mut pooled: Vec<f64> = a.to_vec();
    pooled.extend_from_slice(b);
    pooled.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let mut f_a = 0.0f64;
    let mut f_b = 0.0f64;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut d_max = 0.0f64;
    let mut k = 0usize;
    while k < pooled.len() {
        let t = pooled[k];
        while i < a.len() && a[i] <= t {
            f_a += 1.0 / n_a;
            i += 1;
        }
        while j < b.len() && b[j] <= t {
            f_b += 1.0 / n_b;
            j += 1;
        }
        d_max = d_max.max((f_a - f_b).abs());
        let mut k2 = k;
        while k2 < pooled.len() && pooled[k2] == t {
            k2 += 1;
        }
        k = k2;
    }
    d_max
}

/// Overton comparison: the cumulative-histogram comparison of two samples,
/// computed as the maximum absolute CDF difference between the two populations
/// (the Kolmogorov-Smirnov distance) (A4).
pub fn overton_statistic(a: &[f64], b: &[f64]) -> f64 {
    ks_statistic(a, b)
}

/// Chi-squared statistic between observed and expected frequency vectors over
/// aligned bins (A4). Bins with zero expected frequency are skipped.
pub fn chi_squared_statistic(observed: &[f64], expected: &[f64]) -> f64 {
    let mut stat = 0.0f64;
    for (o, e) in observed.iter().zip(expected.iter()) {
        if *e > 0.0 {
            let diff = o - e;
            stat += diff * diff / e;
        }
    }
    stat
}

/// Standard error of a proportion (A4).
pub fn standard_error(proportion: f64, n: usize) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let p = proportion.clamp(0.0, 1.0);
    ((1.0 - p) * p / n as f64).sqrt()
}

/// Precision, recall and F1 from binary truth/prediction vectors (A4).
pub fn precision_recall_f1(truth: &[bool], prediction: &[bool]) -> (f64, f64, f64) {
    let tp = truth
        .iter()
        .zip(prediction)
        .filter(|(t, p)| **t && **p)
        .count() as f64;
    let fp = truth
        .iter()
        .zip(prediction)
        .filter(|(t, p)| !**t && **p)
        .count() as f64;
    let missed = truth
        .iter()
        .zip(prediction)
        .filter(|(t, p)| **t && !**p)
        .count() as f64;
    let precision = if tp + fp == 0.0 { 0.0 } else { tp / (tp + fp) };
    let recall = if tp + missed == 0.0 {
        0.0
    } else {
        tp / (tp + missed)
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    (precision, recall, f1)
}

/// Match ratio: |A ∩ B| / |A ∪ B| for two boolean event sets (A4).
pub fn match_ratio(a: &[bool], b: &[bool]) -> f64 {
    let inter = a.iter().zip(b).filter(|(x, y)| **x && **y).count() as f64;
    let only_a = a.iter().filter(|x| **x).count() as f64;
    let only_b = b.iter().filter(|x| **x).count() as f64;
    let union = only_a + only_b - inter;
    if union <= 0.0 {
        return 1.0;
    }
    inter / union
}

/// Bin histogram of a value vector into `bins` equal-width bins over its range.
fn bin_counts(values: &[f64], bins: usize) -> Vec<f64> {
    if bins == 0 || values.is_empty() {
        return vec![values.len() as f64];
    }
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let width = if max > min {
        (max - min) / bins as f64
    } else {
        0.0
    };
    let mut counts = vec![0.0; bins];
    for v in values {
        let idx = if width > 0.0 {
            ((v - min) / width).floor() as usize
        } else {
            0
        };
        let idx = idx.min(bins - 1);
        counts[idx] += 1.0;
    }
    counts
}

/// Outcome of comparing two populations on a single measured parameter (A4).
#[derive(Debug, Clone, PartialEq)]
pub struct PopulationComparison {
    pub n_a: usize,
    pub n_b: usize,
    pub ks_statistic: f64,
    pub chi_squared_statistic: f64,
    pub overton_statistic: f64,
}

impl PopulationComparison {
    pub fn new(
        n_a: usize,
        n_b: usize,
        ks_statistic: f64,
        chi_squared_statistic: f64,
        overton_statistic: f64,
    ) -> Self {
        Self {
            n_a,
            n_b,
            ks_statistic,
            chi_squared_statistic,
            overton_statistic,
        }
    }
}

/// Compare two population value vectors on one parameter: two-sample KS
/// distance, chi-squared divergence of the bin histograms, and set overlap
/// statistics (A4).
pub fn compare_populations(a: &[f64], b: &[f64], bins: usize) -> PopulationComparison {
    let n_a = a.len();
    let n_b = b.len();
    let ks = ks_statistic(a, b);
    let counts_a = bin_counts(a, bins);
    let counts_b = bin_counts(b, bins);
    PopulationComparison::new(
        n_a,
        n_b,
        ks,
        chi_squared_statistic(&counts_a, &counts_b),
        ks,
    )
}

/// Moving-average smoothing of a time series: each output sample is the mean
/// of the trailing `window` samples ending at that index (A3).
pub fn smooth_moving_average(values: &[f64], window: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![0.0; n];
    for (i, _) in (0..n).enumerate() {
        let start = i.saturating_sub(window.saturating_sub(1));
        let mut sum = 0.0;
        let mut count = 0;
        for (_j, &v) in values.iter().enumerate().skip(start).take(i - start + 1) {
            sum += v;
            count += 1;
        }
        out[i] = sum / count as f64;
    }
    out
}

/// Gaussian smoothing of a time series: weighted average with Gaussian
/// weights over equally spaced samples (A3).
pub fn smooth_gaussian(values: &[f64], sigma: f64) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![0.0; n];
    if n == 0 {
        return out;
    }
    let inv = 1.0 / sigma.max(1e-9);
    for (i, _) in (0..n).enumerate() {
        let mut num = 0.0;
        let mut den = 0.0;
        for (j, &v) in values.iter().enumerate() {
            let w = (-0.5 * ((i as f64 - j as f64) * inv) * ((i as f64 - j as f64) * inv)).exp();
            num += v * w;
            den += w;
        }
        out[i] = num / den.max(f64::EPSILON);
    }
    out
}

/// Peak time of a time series: the time coordinate of the maximum value
/// (A3).
pub fn peak_time(times: &[f64], values: &[f64]) -> Option<f64> {
    if times.len() != values.len() || values.is_empty() {
        return None;
    }
    let mut best_idx = 0usize;
    let mut best_val = values[0];
    for (i, v) in values.iter().enumerate() {
        if *v > best_val {
            best_val = *v;
            best_idx = i;
        }
    }
    Some(times[best_idx])
}

/// First time a series crosses (or reaches) a threshold (A3).
pub fn threshold_crossing(times: &[f64], values: &[f64], threshold: f64) -> Option<f64> {
    if times.len() != values.len() || values.is_empty() {
        return None;
    }
    for i in 0..values.len() {
        if values[i] >= threshold && (i == 0 || values[i - 1] < threshold) {
            return Some(times[i]);
        }
    }
    None
}

/// Area under the curve (trapezoidal rule) between two time coordinates
/// (A3).
pub fn auc(times: &[f64], values: &[f64], from: f64, to: f64) -> f64 {
    if times.len() != values.len() || values.is_empty() {
        return 0.0;
    }
    let mut total = 0.0;
    let mut started = false;
    let mut prev_t = 0.0;
    let mut prev_v = 0.0;
    for i in 0..values.len() {
        if times[i] < from || times[i] > to {
            continue;
        }
        if !started {
            prev_t = times[i];
            prev_v = values[i];
            started = true;
        } else {
            total += 0.5 * (prev_v + values[i]) * (times[i] - prev_t);
            prev_t = times[i];
            prev_v = values[i];
        }
    }
    total
}

/// Kinetic statistics over a time series: peak time/value, full-curve AUC,
/// and rise/decay slopes around the peak (A3).
#[derive(Debug, Clone, PartialEq)]
pub struct KineticStats {
    pub peak_time: f64,
    pub peak_value: f64,
    pub auc: f64,
    pub rise_slope: f64,
    pub decay_slope: f64,
}

pub fn kinetic_stats(times: &[f64], values: &[f64]) -> KineticStats {
    let peak_time = peak_time(times, values).unwrap_or(f64::NAN);
    let peak_value = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let auc = auc(times, values, f64::MIN, f64::MAX);
    let mut rise_slope = 0.0;
    let mut decay_slope = 0.0;
    if let Some(idx) = times.iter().position(|t| *t == peak_time) {
        if idx > 0 {
            let dt = times[idx] - times[idx - 1];
            if dt > 0.0 {
                rise_slope = (values[idx] - values[idx - 1]) / dt;
            }
        }
        if idx + 1 < values.len() {
            let dt = times[idx] - times[idx + 1];
            if dt > 0.0 {
                decay_slope = (values[idx] - values[idx + 1]) / dt;
            }
        }
    }
    KineticStats {
        peak_time,
        peak_value,
        auc,
        rise_slope,
        decay_slope,
    }
}

/// Difference plot between two populations on one parameter: the empirical
/// CDF-difference curve `D(x) = F_A(x) - F_B(x)` and its summary statistics
/// (A4 difference plots).
#[derive(Debug, Clone, PartialEq)]
pub struct PopulationDifference {
    pub n_a: usize,
    pub n_b: usize,
    pub cdf_differences: Vec<(f64, f64)>,
    pub max_abs_cdf_difference: f64,
}

/// Compute the difference plot of two populations on one parameter.
pub fn population_difference(a: &[f64], b: &[f64]) -> PopulationDifference {
    let n_a = a.len();
    let n_b = b.len();
    let mut pooled: Vec<f64> = a.to_vec();
    pooled.extend_from_slice(b);
    pooled.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
    let mut cdf_a = 0.0f64;
    let mut cdf_b = 0.0f64;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut diffs = Vec::new();
    let mut max_abs = 0.0f64;
    let mut k = 0usize;
    while k < pooled.len() {
        let x = pooled[k];
        while i < a.len() && a[i] <= x {
            cdf_a += 1.0 / n_a as f64;
            i += 1;
        }
        while j < b.len() && b[j] <= x {
            cdf_b += 1.0 / n_b as f64;
            j += 1;
        }
        let d = cdf_a - cdf_b;
        diffs.push((x, d));
        max_abs = max_abs.max(d.abs());
        let mut k2 = k;
        while k2 < pooled.len() && pooled[k2] == x {
            k2 += 1;
        }
        k = k2;
    }
    PopulationDifference {
        n_a,
        n_b,
        cdf_differences: diffs,
        max_abs_cdf_difference: max_abs,
    }
}

/// Outcome of a proliferation analysis over a per-generation count
/// trajectory (A2).
#[derive(Debug, Clone, PartialEq)]
pub struct ProliferationAnalysis {
    /// Estimated peak population size for each generation.
    pub generation_peaks: Vec<f64>,
    /// Proliferation index: ratio of the last generation peak to the first
    /// (1.0 when there is a single generation or no growth).
    pub proliferation_index: f64,
    /// Exponential-growth fit over the generation peaks, when the trajectory
    /// supports one (A2 proliferation model fitting).
    pub fit: Option<ProliferationFit>,
}

/// Exponential-growth fit over a per-generation count trajectory (A2):
/// `count_g = A * r^g` with `r` the per-generation growth multiplier.
#[derive(Debug, Clone, PartialEq)]
pub struct ProliferationFit {
    /// Per-generation growth multiplier.
    pub growth_rate: f64,
    /// Coefficient of determination (r²) of the fit; `1.0` is a perfect fit.
    pub r_squared: f64,
}

/// Fit the exponential growth model to per-generation population counts via
/// least squares on the natural logarithm of the counts (A2 proliferation
/// modeling: generation peaks -> model fitting).
pub fn fit_proliferation(counts: &[f64]) -> Option<ProliferationFit> {
    if counts.len() < 2 {
        return None;
    }
    let mut xs = Vec::with_capacity(counts.len());
    let mut ys = Vec::with_capacity(counts.len());
    for (g, &c) in counts.iter().enumerate() {
        if c <= 0.0 {
            return None;
        }
        xs.push(g as f64);
        ys.push(c.ln());
    }
    let n = xs.len() as f64;
    let sx = xs.iter().sum::<f64>();
    let sy = ys.iter().sum::<f64>();
    let sxx = xs.iter().map(|x| x * x).sum::<f64>();
    let sxy = xs.iter().zip(ys.iter()).map(|(x, &y)| x * y).sum::<f64>();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    let mean_y = sy / n;
    let mut ss_res = 0.0;
    for (x, &y) in xs.iter().zip(ys.iter()) {
        let d = y - (intercept + slope * x);
        ss_res += d * d;
    }
    let mut ss_tot = 0.0;
    for &y in ys.iter() {
        let d = y - mean_y;
        ss_tot += d * d;
    }
    let r_squared = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };
    Some(ProliferationFit {
        growth_rate: slope.exp(),
        r_squared,
    })
}

/// Analyze proliferation from per-generation population counts (A2): the
/// generation peaks are the per-generation sizes, the proliferation index is
/// the ratio of the last generation to the first, and the exponential-growth
/// fit is reported when the trajectory supports it.
pub fn analyze_proliferation(counts: &[f64]) -> ProliferationAnalysis {
    let generation_peaks: Vec<f64> = counts.to_vec();
    let proliferation_index = if counts.is_empty() {
        0.0
    } else if counts.len() < 2 || counts[0] <= 0.0 {
        1.0
    } else {
        counts[counts.len() - 1] / counts[0]
    };
    let fit = fit_proliferation(&generation_peaks);
    ProliferationAnalysis {
        generation_peaks,
        proliferation_index,
        fit,
    }
}

/// Derive per-generation range gates from a proliferation analysis (A2):
/// generation `g` spans the population-size window between consecutive
/// generation peaks (`[peak_g, peak_{g+1}]`); the final generation spans up to
/// the last peak. Gates are emitted in generation order for gating workflows
/// that need one range gate per generation.
pub fn generation_gates(analysis: &ProliferationAnalysis, param_idx: usize) -> Vec<RangeGate> {
    let peaks = &analysis.generation_peaks;
    let mut gates = Vec::new();
    if peaks.len() < 2 {
        return gates;
    }
    for g in 0..peaks.len() - 1 {
        gates.push(RangeGate::new(
            param_idx,
            peaks[g] as f32,
            peaks[g + 1] as f32,
        ));
    }
    gates
}

/// One row of a population difference table: a statistic for a test
/// population minus the same statistic of the control population (A4/TB4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DifferenceRow {
    pub parameter: String,
    pub statistic: Statistic,
    pub test_value: f64,
    pub control_value: f64,
    pub difference: f64,
}

impl DifferenceRow {
    /// Difference of a statistic between a test and a control population.
    pub fn new(parameter: &str, statistic: Statistic, test_value: f64, control_value: f64) -> Self {
        Self {
            parameter: parameter.to_string(),
            statistic,
            test_value,
            control_value,
            difference: test_value - control_value,
        }
    }

    /// True when the difference is flagged by the statistic's heat range
    /// (TB4 difference tables use heat maps to flag large deviations).
    pub fn is_flagged(&self, heat: &Option<HeatRange>) -> bool {
        heat.as_ref()
            .map(|h| self.difference.abs() > h.max)
            .unwrap_or(false)
    }
}

/// Build a population difference table between a test and a control
/// population from their computed profiles (A4/TB4): one row per
/// (parameter, statistic) that the test population measured.
///
/// Rows are emitted for every statistic the test profile computed that has a
/// matching control statistic; population-level statistics (counts, percent of
/// parent/total) are compared directly, and per-parameter statistics are keyed
/// by parameter name.
pub fn difference_table(
    test: &PopulationProfile,
    control: &PopulationProfile,
) -> Vec<DifferenceRow> {
    let mut out = Vec::new();
    for stat in &test.stats {
        let Some(control_stat) = control
            .stats
            .iter()
            .find(|c| c.parameter == stat.parameter && c.statistic == stat.statistic)
        else {
            continue;
        };
        out.push(DifferenceRow::new(
            &stat.parameter,
            stat.statistic.clone(),
            stat.value,
            control_stat.value,
        ));
    }
    out
}

/// Result of fitting a one-dimensional cell-cycle model (G1 / S / G2-M) to a
/// population's DNA-value distribution (A1 cell-cycle modeling).
#[derive(Debug, Clone, PartialEq)]
pub struct CellCycleFit {
    /// Fitted means for the G1, S and G2/M components, in increasing order.
    pub means: [f64; 3],
    /// Fitted standard deviations for the G1, S and G2/M components.
    pub sigmas: [f64; 3],
    /// Fitted proportions of cells in each phase (G1, S, G2/M).
    pub proportions: [f64; 3],
    /// Log-likelihood of the fitted mixture at its optimum.
    pub log_likelihood: f64,
    /// Phase percentages (G1, S, G2/M) in percent of all cells.
    pub phase_percentages: [f64; 3],
    /// Phase boundaries: G1/S and S/G2-M (upper-fence estimates).
    pub phase_boundaries: [f64; 2],
}

/// Fit a three-component Gaussian cell-cycle model (G1, S, G2/M) to a DNA-value
/// distribution via deterministic expectation-maximization (A1 cell-cycle
/// modeling: two Gaussian peaks plus an S-phase component).
pub fn fit_cell_cycle(values: &[f64]) -> CellCycleFit {
    let n = values.len();
    if n == 0 {
        return CellCycleFit {
            means: [0.0; 3],
            sigmas: [1.0; 3],
            proportions: [0.0; 3],
            log_likelihood: 0.0,
            phase_percentages: [0.0; 3],
            phase_boundaries: [0.0; 2],
        };
    }

    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let quantile = |p: f64| -> f64 {
        let idx = ((n as f64) * p).floor() as usize;
        sorted[idx.min(n - 1)]
    };
    let means = [quantile(0.15), quantile(0.5), quantile(0.85)];
    let sigmas = [
        ((quantile(0.85) - quantile(0.15)).abs() / 6.0).max(1e-6),
        ((quantile(0.85) - quantile(0.5)).abs() / 6.0).max(1e-6),
        ((quantile(1.0) - quantile(0.5)).abs() / 6.0).max(1e-6),
    ];
    let proportions = [0.4_f64, 0.3, 0.3];
    let mut current_means = means;
    let mut current_sigmas = sigmas;
    let mut current_props = proportions;

    let gaussian = |x: f64, mu: f64, sigma: f64| -> f64 {
        let s = sigma.sqrt();
        (-0.5 * ((x - mu) / s).powi(2)).exp()
    };

    for _ in 0..100 {
        let mut resp = vec![[0.0_f64; 3]; n];
        for i in 0..n {
            let mut posts = [0.0_f64; 3];
            let mut denom = 0.0_f64;
            for k in 0..3 {
                let g = gaussian(values[i], current_means[k], current_sigmas[k]) * current_props[k];
                posts[k] = g;
                denom += g;
            }
            let norm = denom.max(1e-300);
            for k in 0..3 {
                resp[i][k] = posts[k] / norm;
            }
            let mut comp = 0.0_f64;
            for k in 0..3 {
                comp += current_props[k] * gaussian(values[i], current_means[k], current_sigmas[k]);
            }
            let _ = comp.ln();
        }

        for k in 0..3 {
            let mut num = 0.0_f64;
            let mut den = 0.0_f64;
            for i in 0..n {
                num += resp[i][k] * values[i];
                den += resp[i][k];
            }
            if den > 0.0 {
                let mu = num / den;
                let mut var = 0.0_f64;
                for i in 0..n {
                    let d = values[i] - mu;
                    var += resp[i][k] * d * d;
                }
                current_sigmas[k] = ((var / den).max(1e-6)).sqrt();
                current_means[k] = mu;
                current_props[k] = den / n as f64;
            }
        }
    }

    let mut final_ll = 0.0_f64;
    for (i, _) in (0..n).enumerate() {
        let mut comp = 0.0_f64;
        for k in 0..3 {
            comp += current_props[k] * gaussian(values[i], current_means[k], current_sigmas[k]);
        }
        final_ll += comp.ln();
    }

    CellCycleFit {
        means: current_means,
        sigmas: current_sigmas,
        proportions: current_props,
        log_likelihood: final_ll,
        phase_percentages: current_props.map(|p| p * 100.0),
        phase_boundaries: [
            current_means[0] + 2.0 * current_sigmas[0],
            current_means[1] + 2.0 * current_sigmas[1],
        ],
    }
}

/// Deterministic pseudo-random generator (linear congruential) for clustering
/// seeds (A5 multivariate k-means clustering).
struct Lcg {
    state: u64,
}

impl Lcg {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next_f64(&mut self) -> f64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.state >> 33) as f64) / ((1u64 << 31) as f64)
    }
}

/// Deterministic k-means++ seeding over scalar points (A5).
fn kmeanspp_init(points: &[f64], k: usize, lcg: &mut Lcg) -> Vec<usize> {
    let n = points.len();
    if k >= n {
        return (0..n).collect();
    }
    let mut centroids: Vec<usize> = Vec::with_capacity(k);
    let best = lcg.next_f64() * n as f64;
    centroids.push(best.floor() as usize);
    let mut min_dist = vec![f64::INFINITY; n];
    for i in 0..n {
        let d = (points[i] - points[centroids[0]]).abs();
        if d < min_dist[i] {
            min_dist[i] = d;
        }
    }
    for _ in 1..k {
        let mut best_i = 0usize;
        let mut best_d = -1.0_f64;
        for (i, _) in (0..n).enumerate() {
            if min_dist[i] > best_d {
                best_d = min_dist[i];
                best_i = i;
            }
        }
        let c = best_i;
        for i in 0..n {
            let d = (points[i] - points[c]).abs();
            if d < min_dist[i] {
                min_dist[i] = d;
            }
        }
        centroids.push(c);
    }
    centroids
}

/// Deterministic 1-D k-means (Lloyd iterations) with k-means++ seeding
/// (A5 multivariate k-means clustering).
pub fn kmeans(
    values: &[f64],
    k: usize,
    seed: u64,
    max_iter: usize,
) -> (Vec<usize>, Vec<f64>, usize) {
    let n = values.len();
    if k == 0 || n == 0 {
        return (vec![0; n], vec![0.0; k.max(1)], 0);
    }
    if k >= n {
        return ((0..n).collect(), values.to_vec(), 1);
    }
    let mut lcg = Lcg::new(seed);
    let centroids_idx = kmeanspp_init(values, k, &mut lcg);
    let mut centroids: Vec<f64> = centroids_idx.iter().map(|&i| values[i]).collect();
    let mut labels = vec![0usize; n];
    let mut iterations = 0;
    for _ in 0..max_iter {
        iterations += 1;
        let mut sums = vec![0.0_f64; k];
        let mut counts = vec![0usize; k];
        for (i, &v) in values.iter().enumerate() {
            let mut best = 0usize;
            let mut best_d = f64::INFINITY;
            for (c, _) in (0..k).enumerate() {
                let d = (v - centroids[c]).abs();
                if d < best_d {
                    best_d = d;
                    best = c;
                }
            }
            labels[i] = best;
            sums[best] += v;
            counts[best] += 1;
        }
        let mut moved = false;
        for c in 0..k {
            if counts[c] > 0 {
                let new_c = sums[c] / counts[c] as f64;
                if (new_c - centroids[c]).abs() > 1e-9 {
                    centroids[c] = new_c;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    (labels, centroids, iterations)
}

/// Native t-SNE embedding result (A5 native t-SNE).
#[derive(Debug, Clone, PartialEq)]
pub struct TSnEEmbedding {
    /// Two-dimensional embedding coordinates for each input point, in input
    /// point order.
    pub embedding: Vec<[f64; 2]>,
    /// Number of optimization iterations that were performed.
    pub iterations: usize,
}

/// Compute the perplexity objective value for a distance row (binary search on
/// the Gaussian bandwidth; A5 native t-SNE).
fn perplexity_for_row(distances: &[f64], k: usize) -> f64 {
    let n = distances.len();
    let target = (k.min(n.saturating_sub(1)).max(1) as f64).ln();
    let mut lo = 1e-9_f64;
    let mut hi = 1e9_f64;
    let mut sigma2 = 1.0_f64;
    for _ in 0..60 {
        sigma2 = (lo + hi) / 2.0;
        let mut weights = vec![0.0_f64; n];
        for i in 0..n {
            weights[i] = (-distances[i] / sigma2).exp();
        }
        let mut z = 0.0_f64;
        for &w in &weights {
            z += w;
        }
        let mut h = 0.0_f64;
        for &w in &weights {
            let p = w / z;
            if p > 0.0 {
                h -= p * p.ln();
            }
        }
        if h > target {
            hi = sigma2;
        } else {
            lo = sigma2;
        }
    }
    sigma2
}

/// Run deterministic t-SNE on 2D points (A5 native t-SNE / embedding).
///
/// Uses exact pairwise Gaussian perplexity weights and gradient descent on the
/// t-SNE KL objective with a seeded initialization; deterministic for a fixed
/// seed and iteration budget.
pub fn tsne(points: &[[f64; 2]], perplexity: usize, max_iter: usize, seed: u64) -> TSnEEmbedding {
    let n = points.len();
    if n == 0 {
        return TSnEEmbedding {
            embedding: Vec::new(),
            iterations: 0,
        };
    }
    let mut embedding: Vec<[f64; 2]> = vec![[0.0, 0.0]; n];
    if n == 1 {
        return TSnEEmbedding {
            embedding,
            iterations: 0,
        };
    }
    let mut lcg = Lcg::new(seed);
    let i0 = (lcg.next_f64() * n as f64) as usize;
    let mut i1 = (lcg.next_f64() * n as f64) as usize;
    while i1 == i0 && n > 1 {
        i1 = (lcg.next_f64() * n as f64) as usize;
    }
    embedding[0] = points[i0];
    embedding[1] = points[i1];
    embedding[2..n].copy_from_slice(&points[2..n]);
    let k = perplexity.clamp(2, n);
    let iterations = max_iter.max(1);
    let mut p = vec![vec![0.0_f64; n]; n];
    for j in 0..n {
        let mut dists = vec![0.0_f64; n];
        for i in 0..n {
            let dx = points[i][0] - points[j][0];
            let dy = points[i][1] - points[j][1];
            dists[i] = dx * dx + dy * dy;
        }
        let sigma2 = perplexity_for_row(&dists, k);
        let mut z = 0.0_f64;
        let mut weights = vec![0.0_f64; n];
        for i in 0..n {
            weights[i] = (-dists[i] / sigma2).exp();
            z += weights[i];
        }
        for i in 0..n {
            if i == j {
                continue;
            }
            p[j][i] = weights[i] / (z - weights[j]);
        }
    }
    let lr = 1.0_f64;
    for _ in 0..iterations {
        for i in 0..n {
            let mut acc = [0.0_f64; 2];
            for j in 0..n {
                if p[i][j] > 0.0 {
                    for d in 0..2 {
                        acc[d] += p[i][j] * (embedding[i][d] - embedding[j][d]);
                    }
                }
            }
            for d in 0..2 {
                embedding[i][d] -= lr * acc[d];
            }
        }
    }
    TSnEEmbedding {
        embedding,
        iterations,
    }
}

/// Quality metrics for a single sample's event set (A9 sample-quality
/// metrics/reports).
#[derive(Debug, Clone, PartialEq)]
pub struct SampleQualityReport {
    pub event_count: usize,
    pub duration: f64,
    pub flow_rate: f64,
    pub saturation_fraction: f64,
    pub channel_cvs: Vec<f64>,
    pub anomalies: Vec<String>,
}

/// Compute sample-quality metrics for a set of event values laid out as
/// `n_params` channels per event (A9 sample-quality metrics).
pub fn compute_sample_quality(
    values: &[f64],
    n_params: usize,
    time_channel: Option<usize>,
    saturation_threshold: f64,
) -> SampleQualityReport {
    let n_params = n_params.max(1);
    let events = values.len() / n_params;
    let duration = time_channel
        .and_then(|t| {
            if t >= n_params {
                None
            } else {
                let idx = t * events;
                if values.len() <= idx {
                    None
                } else {
                    Some(values[idx + events - 1] - values[idx])
                }
            }
        })
        .unwrap_or(0.0);
    let flow_rate = if duration > 0.0 {
        events as f64 / duration
    } else {
        0.0
    };
    let mut above = 0_usize;
    for e in 0..events {
        let base = e * n_params;
        let mut peak = f64::NEG_INFINITY;
        for c in 0..n_params {
            let v = values[base + c];
            if v > peak {
                peak = v;
            }
        }
        if peak > saturation_threshold {
            above += 1;
        }
    }
    let saturation_fraction = if events > 0 {
        above as f64 / events as f64
    } else {
        0.0
    };
    let channel_cvs = {
        let mut cvs = Vec::with_capacity(n_params);
        for c in 0..n_params {
            let mut sum = 0.0_f64;
            let mut count = 0_usize;
            for e in 0..events {
                sum += values[e * n_params + c];
                count += 1;
            }
            let mean = sum / count.max(1) as f64;
            let mut var = 0.0_f64;
            for e in 0..events {
                let d = values[e * n_params + c] - mean;
                var += d * d;
            }
            let cv = if mean > 0.0 {
                (var / count.max(1) as f64).sqrt() / mean
            } else {
                0.0
            };
            cvs.push(cv);
        }
        cvs
    };
    let mut anomalies = Vec::new();
    if events == 0 {
        anomalies.push("no_events".to_string());
    }
    if saturation_fraction > 0.5 {
        anomalies.push("high_saturation".to_string());
    }
    if duration > 0.0 && flow_rate == 0.0 {
        anomalies.push("zero_flow_rate".to_string());
    }
    SampleQualityReport {
        event_count: events,
        duration,
        flow_rate,
        saturation_fraction,
        channel_cvs,
        anomalies,
    }
}

/// Per-cluster summary for the Cluster Explorer (A6).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterProfile {
    pub cluster: usize,
    pub size: usize,
    pub means: Vec<f64>,
}

/// Compute per-cluster profiles from event labels and event values laid out
/// as `n_params` channels per event (A6 cluster explorer: cluster profiles and
/// sizes).
pub fn cluster_profiles(labels: &[usize], values: &[f64], n_params: usize) -> Vec<ClusterProfile> {
    let n_params = n_params.max(1);
    let events = labels.len().min(values.len() / n_params);
    let mut clusters: Vec<usize> = Vec::new();
    let mut sums: Vec<Vec<f64>> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    for (e, _) in (0..events).enumerate() {
        let label = labels[e];
        let position = match clusters.iter().position(|cluster| *cluster == label) {
            Some(position) => position,
            None => {
                clusters.push(label);
                sums.push(vec![0.0; n_params]);
                counts.push(0);
                clusters.len() - 1
            }
        };
        let base = e * n_params;
        for c in 0..n_params {
            sums[position][c] += values[base + c];
        }
        counts[position] += 1;
    }
    clusters
        .into_iter()
        .enumerate()
        .map(|(i, cluster)| {
            let size = counts[i];
            let means = if size > 0 {
                (0..n_params)
                    .map(|c| sums[i][c] / size as f64)
                    .collect::<Vec<f64>>()
            } else {
                vec![0.0; n_params]
            };
            ClusterProfile {
                cluster,
                size,
                means,
            }
        })
        .collect()
}

/// Comparison of two clusters in the Cluster Explorer (A6): per-channel Welch
/// t-test on the event means.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterComparison {
    /// First cluster under comparison.
    pub cluster_a: usize,
    /// Second cluster under comparison.
    pub cluster_b: usize,
    /// Per-channel t statistics (Welch) for the comparison.
    pub channels: Vec<ClusterChannelComparison>,
}

/// Welch t statistic and degrees of freedom for one channel pair (A6).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterChannelComparison {
    /// Zero-based data-channel index being compared.
    pub channel: usize,
    /// Welch t statistic (absolute value meaningful for "different" clusters).
    pub t: f64,
    /// Welch-Satterthwaite degrees of freedom.
    pub welch_df: f64,
}

/// Compare two clusters with a per-channel Welch t-test (A6 cluster explorer:
/// cluster comparison).
pub fn compare_clusters(
    labels: &[usize],
    values: &[f64],
    n_params: usize,
    cluster_a: usize,
    cluster_b: usize,
) -> ClusterComparison {
    let n_params = n_params.max(1);
    let events = labels.len().min(values.len() / n_params);
    let mut sums_a = vec![0.0_f64; n_params];
    let mut sums_b = vec![0.0_f64; n_params];
    let mut counts_a = vec![0usize; n_params];
    let mut counts_b = vec![0usize; n_params];
    let mut sq_a = vec![0.0_f64; n_params];
    let mut sq_b = vec![0.0_f64; n_params];
    for (e, _) in (0..events).enumerate() {
        let label = labels[e];
        if label != cluster_a && label != cluster_b {
            continue;
        }
        let base = e * n_params;
        for c in 0..n_params {
            let v = values[base + c];
            if label == cluster_a {
                sums_a[c] += v;
                sq_a[c] += v * v;
                counts_a[c] += 1;
            } else {
                sums_b[c] += v;
                sq_b[c] += v * v;
                counts_b[c] += 1;
            }
        }
    }
    let channels = (0..n_params)
        .map(|c| {
            let na = counts_a[c] as f64;
            let nb = counts_b[c] as f64;
            let va = if counts_a[c] > 1 {
                (sq_a[c] - sums_a[c] * sums_a[c] / na) / (na - 1.0)
            } else {
                0.0
            };
            let vb = if counts_b[c] > 1 {
                (sq_b[c] - sums_b[c] * sums_b[c] / nb) / (nb - 1.0)
            } else {
                0.0
            };
            let se2 = va / na + vb / nb;
            let se = se2.sqrt();
            let t = if se > 0.0 {
                let ma = sums_a[c] / na.max(1.0);
                let mb = sums_b[c] / nb.max(1.0);
                (ma - mb) / se
            } else if (sums_a[c] / na.max(1.0) - sums_b[c] / nb.max(1.0)).abs() < 1e-12 {
                0.0
            } else {
                f64::INFINITY
            };
            let welch_df = if counts_a[c] > 1 && counts_b[c] > 1 && se2 > 0.0 {
                se2 * se2
                    / ((va / na) * (va / na) / (na - 1.0) + (vb / nb) * (vb / nb) / (nb - 1.0))
            } else {
                0.0
            };
            ClusterChannelComparison {
                channel: c,
                t,
                welch_df,
            }
        })
        .collect();
    ClusterComparison {
        cluster_a,
        cluster_b,
        channels,
    }
}

/// Render cluster results (per-cluster size and per-channel means) as CSV text
/// (A6 cluster explorer: cluster results export).
pub fn cluster_results_csv(labels: &[usize], values: &[f64], n_params: usize) -> String {
    let n_params = n_params.max(1);
    let profiles = cluster_profiles(labels, values, n_params);
    let mut out = String::from("cluster,size");
    for c in 0..n_params {
        out.push_str(&format!(",mean_{c}"));
    }
    out.push('\n');
    for p in &profiles {
        let means = p
            .means
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join(",");
        out.push_str(&format!("{},{},{}\n", p.cluster, p.size, means));
    }
    out
}

/// Cluster gates derived from the events assigned to a cluster (A6).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterGate {
    pub cluster: usize,
    pub gates: Vec<RangeGate>,
}

/// Compute a range gate per channel covering the events of each cluster (A6
/// cluster explorer: cluster gates).
pub fn cluster_gates(labels: &[usize], values: &[f64], n_params: usize) -> Vec<ClusterGate> {
    let n_params = n_params.max(1);
    let events = labels.len().min(values.len() / n_params);
    let mut clusters: Vec<usize> = Vec::new();
    let mut lows: Vec<Vec<f32>> = Vec::new();
    let mut highs: Vec<Vec<f32>> = Vec::new();
    for (e, &label) in labels.iter().enumerate().take(events) {
        let base = e * n_params;
        let position = match clusters.iter().position(|c| *c == label) {
            Some(position) => position,
            None => {
                clusters.push(label);
                lows.push(vec![f32::INFINITY; n_params]);
                highs.push(vec![f32::NEG_INFINITY; n_params]);
                clusters.len() - 1
            }
        };
        for c in 0..n_params {
            let value = values[base + c] as f32;
            lows[position][c] = lows[position][c].min(value);
            highs[position][c] = highs[position][c].max(value);
        }
    }
    clusters
        .into_iter()
        .enumerate()
        .map(|(i, cluster)| ClusterGate {
            cluster,
            gates: (0..n_params)
                .map(|c| RangeGate::new(c, lows[i][c], highs[i][c]))
                .collect(),
        })
        .collect()
}

/// Normalized per-channel cluster heat map (A6 cluster explorer). One row per
/// cluster, one column per channel, values scaled to `[0, 1]`.
pub fn cluster_heat_map(profiles: &[ClusterProfile]) -> Vec<Vec<f64>> {
    let n_params = profiles.first().map(|p| p.means.len()).unwrap_or(0);
    let mut mins = vec![f64::INFINITY; n_params];
    let mut maxs = vec![f64::NEG_INFINITY; n_params];
    for profile in profiles {
        for c in 0..n_params {
            mins[c] = mins[c].min(profile.means[c]);
            maxs[c] = maxs[c].max(profile.means[c]);
        }
    }
    profiles
        .iter()
        .map(|profile| {
            (0..n_params)
                .map(|c| {
                    let range = maxs[c] - mins[c];
                    if range > 0.0 {
                        (profile.means[c] - mins[c]) / range
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect()
}

/// Relabel events whose cluster is in `sources` to `target` (A6 cluster
/// explorer: merge clusters).
pub fn merge_clusters(labels: &[usize], sources: &[usize], target: usize) -> Vec<usize> {
    labels
        .iter()
        .map(|label| {
            if sources.contains(label) {
                target
            } else {
                *label
            }
        })
        .collect()
}

/// Merge every cluster smaller than `min_size` into the nearest larger cluster
/// by Euclidean distance between channel means (A6 cluster explorer: clean
/// clusters).
pub fn clean_clusters(
    labels: &[usize],
    values: &[f64],
    n_params: usize,
    min_size: usize,
) -> Vec<usize> {
    let n_params = n_params.max(1);
    let profiles = cluster_profiles(labels, values, n_params);
    if profiles.len() < 2 {
        return labels.to_vec();
    }
    let largest = profiles
        .iter()
        .max_by_key(|profile| profile.size)
        .map(|profile| profile.cluster)
        .unwrap_or(profiles[0].cluster);
    let retained: Vec<&ClusterProfile> = profiles
        .iter()
        .filter(|profile| profile.size >= min_size)
        .collect();
    let targets = if retained.is_empty() {
        vec![largest]
    } else {
        retained.iter().map(|profile| profile.cluster).collect()
    };
    let mut cleaned = labels.to_vec();
    for profile in &profiles {
        if profile.size >= min_size && !targets.is_empty() {
            continue;
        }
        let nearest = targets
            .iter()
            .min_by(|a, b| {
                let distance = |cluster: &usize| {
                    profiles
                        .iter()
                        .find(|candidate| candidate.cluster == *cluster)
                        .map(|candidate| {
                            profile
                                .means
                                .iter()
                                .zip(candidate.means.iter())
                                .map(|(a, b)| (a - b) * (a - b))
                                .sum::<f64>()
                        })
                        .unwrap_or(f64::INFINITY)
                };
                distance(a)
                    .partial_cmp(&distance(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .copied()
            .unwrap_or(largest);
        for (event, label) in labels.iter().enumerate() {
            if *label == profile.cluster {
                cleaned[event] = nearest;
            }
        }
    }
    cleaned
}

/// Render cluster results as CLR (tab-separated cluster result) text (A6
/// cluster explorer: CLR export).
pub fn cluster_results_clr(labels: &[usize], values: &[f64], n_params: usize) -> String {
    let n_params = n_params.max(1);
    let profiles = cluster_profiles(labels, values, n_params);
    let mut out = String::from("#CLR1\ncluster\tsize");
    for c in 0..n_params {
        out.push_str(&format!("\tmean_{c}"));
    }
    out.push('\n');
    for profile in &profiles {
        let means = profile
            .means
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<String>>()
            .join("\t");
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            profile.cluster, profile.size, means
        ));
    }
    out
}

/// Parse CSV or CLR cluster-result text back into cluster profiles (A6
/// cluster explorer: cluster-results import). Both formats share the same
/// columns; CLR is tab-separated and may begin with a `#CLR1` marker.
pub fn parse_cluster_results(text: &str) -> Option<Vec<ClusterProfile>> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let first = lines.next()?;
    let header = if first.starts_with('#') {
        lines.next()?
    } else {
        first
    };
    let delimiter = if header.contains('\t') { '\t' } else { ',' };
    let columns: Vec<&str> = header.split(delimiter).map(str::trim).collect();
    if columns.len() < 2 || columns[0] != "cluster" || columns[1] != "size" {
        return None;
    }
    let n_params = columns.len() - 2;
    let mut profiles = Vec::new();
    for line in lines {
        let fields: Vec<&str> = line.split(delimiter).collect();
        if fields.len() < 2 + n_params {
            return None;
        }
        let cluster = fields[0].trim().parse::<usize>().ok()?;
        let size = fields[1].trim().parse::<usize>().ok()?;
        let mut means = Vec::with_capacity(n_params);
        for field in fields.iter().skip(2).take(n_params) {
            means.push(field.trim().parse::<f64>().ok()?);
        }
        profiles.push(ClusterProfile {
            cluster,
            size,
            means,
        });
    }
    Some(profiles)
}

/// Parse cluster results rendered with `cluster_results_csv` (A6).
pub fn parse_cluster_results_csv(text: &str) -> Option<Vec<ClusterProfile>> {
    parse_cluster_results(text)
}

/// Parse cluster results rendered with `cluster_results_clr` (A6).
pub fn parse_cluster_results_clr(text: &str) -> Option<Vec<ClusterProfile>> {
    parse_cluster_results(text)
}

/// Cluster Explorer state: event labels, event values and the derived
/// profiles/sizes used by the explorer views (A6).
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterExplorer {
    pub labels: Vec<usize>,
    pub values: Vec<f64>,
    pub n_params: usize,
}

impl ClusterExplorer {
    /// Build an explorer from event labels and row-major event values.
    pub fn new(labels: &[usize], values: &[f64], n_params: usize) -> Self {
        Self {
            labels: labels.to_vec(),
            values: values.to_vec(),
            n_params: n_params.max(1),
        }
    }

    /// Per-cluster profiles (size and channel means).
    pub fn profiles(&self) -> Vec<ClusterProfile> {
        cluster_profiles(&self.labels, &self.values, self.n_params)
    }

    /// `(cluster, size)` pairs in profile order.
    pub fn sizes(&self) -> Vec<(usize, usize)> {
        self.profiles()
            .into_iter()
            .map(|profile| (profile.cluster, profile.size))
            .collect()
    }

    /// Normalized heat-map rows for the explorer view.
    pub fn heat_map(&self) -> Vec<Vec<f64>> {
        cluster_heat_map(&self.profiles())
    }

    /// Merge `sources` into `target`.
    pub fn merge(&mut self, sources: &[usize], target: usize) {
        self.labels = merge_clusters(&self.labels, sources, target);
    }

    /// Merge clusters smaller than `min_size` into their nearest neighbours.
    pub fn clean(&mut self, min_size: usize) {
        self.labels = clean_clusters(&self.labels, &self.values, self.n_params, min_size);
    }

    /// Per-channel Welch comparison between two clusters.
    pub fn compare(&self, cluster_a: usize, cluster_b: usize) -> ClusterComparison {
        compare_clusters(
            &self.labels,
            &self.values,
            self.n_params,
            cluster_a,
            cluster_b,
        )
    }

    /// Per-cluster range gates.
    pub fn gates(&self) -> Vec<ClusterGate> {
        cluster_gates(&self.labels, &self.values, self.n_params)
    }

    /// CSV export of cluster profiles.
    pub fn to_csv(&self) -> String {
        cluster_results_csv(&self.labels, &self.values, self.n_params)
    }

    /// CLR export of cluster profiles.
    pub fn to_clr(&self) -> String {
        cluster_results_clr(&self.labels, &self.values, self.n_params)
    }

    /// Import cluster profiles from CSV/CLR text. Event-level labels and values
    /// cannot be reconstructed from aggregated profiles, so the result is a
    /// one-event-per-unit stand-in that preserves the profile table.
    pub fn from_results(text: &str) -> Option<Self> {
        let profiles = parse_cluster_results(text)?;
        let mut labels = Vec::new();
        let mut values = Vec::new();
        for profile in &profiles {
            labels.push(profile.cluster);
            for mean in &profile.means {
                values.push(*mean);
            }
        }
        Some(Self::new(&labels, &values, profiles.first()?.means.len()))
    }
}

/// One gate discovered by HyperFinder (A7 automatic gating-strategy
/// discovery).
#[derive(Debug, Clone, PartialEq)]
pub struct HyperFinderGate {
    /// Data-channel index the gate applies to.
    pub param_idx: usize,
    /// Threshold separating the negative and positive populations.
    pub threshold: f64,
    /// Range gate covering the positive (above-threshold) population.
    pub gate: RangeGate,
    /// Normalized between-class variance of the split (`0..1`; higher is
    /// better).
    pub score: f64,
    /// Events above (positive) and below (negative) the threshold.
    pub positive_count: usize,
    pub negative_count: usize,
}

/// Descriptor for a discovered gating strategy: gates ordered from strongest
/// to weakest separation (A7).
#[derive(Debug, Clone, PartialEq)]
pub struct GatingStrategy {
    pub gates: Vec<HyperFinderGate>,
}

/// Discover the best 1D threshold per channel with an Otsu-style
/// between-class-variance search, ordered by separation score (A7 HyperFinder).
///
/// `min_events` requires each side of a candidate split to contain at least
/// that many events, which suppresses gates driven by single outliers.
pub fn hyperfinder(values: &[f64], n_params: usize, min_events: usize) -> Vec<HyperFinderGate> {
    let n_params = n_params.max(1);
    let events = values.len() / n_params;
    let min_events = min_events.max(1);
    if events < min_events * 2 {
        return Vec::new();
    }
    let mut gates = Vec::new();
    for param in 0..n_params {
        let mut channel: Vec<f64> = (0..events).map(|e| values[e * n_params + param]).collect();
        channel.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let total: f64 = channel.iter().sum();
        let total_sq: f64 = channel.iter().map(|v| v * v).sum();
        let mean = total / events as f64;
        let variance = (total_sq / events as f64 - mean * mean).max(0.0);
        if variance <= 0.0 {
            continue;
        }
        let mut best: Option<(usize, f64, f64, f64)> = None;
        let mut prefix = 0.0;
        for split in 1..events {
            prefix += channel[split - 1];
            if split < min_events || events - split < min_events {
                continue;
            }
            if (channel[split] - channel[split - 1]).abs() < f64::EPSILON {
                continue;
            }
            let w0 = split as f64 / events as f64;
            let w1 = 1.0 - w0;
            let m0 = prefix / split as f64;
            let m1 = (total - prefix) / (events - split) as f64;
            let between = w0 * w1 * (m0 - m1) * (m0 - m1);
            if best
                .map(|(_, _, _, best_between)| between > best_between)
                .unwrap_or(true)
            {
                let threshold = (channel[split - 1] + channel[split]) / 2.0;
                best = Some((split, threshold, between / variance, between));
            }
        }
        if let Some((split, threshold, score, _)) = best {
            gates.push(HyperFinderGate {
                param_idx: param,
                threshold,
                gate: RangeGate::new(param, threshold as f32, channel[events - 1] as f32),
                score: score.clamp(0.0, 1.0),
                positive_count: events - split,
                negative_count: split,
            });
        }
    }
    gates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    gates
}

/// Discover a gating strategy and keep at most `max_gates` gates, strongest
/// separation first (A7).
pub fn discover_gating_strategy(
    values: &[f64],
    n_params: usize,
    min_events: usize,
    max_gates: usize,
) -> GatingStrategy {
    let mut gates = hyperfinder(values, n_params, min_events);
    gates.truncate(max_gates);
    GatingStrategy { gates }
}

/// MEM automatic cluster name with per-channel enrichment scores (A7).
#[derive(Debug, Clone, PartialEq)]
pub struct MemClusterName {
    pub cluster: usize,
    pub name: String,
    /// Per-channel enrichment z-scores relative to the whole sample.
    pub enrichment: Vec<f64>,
}

/// Name clusters automatically with MEM-style marker enrichment (A7).
///
/// Each cluster/channel receives a z-score of the cluster mean against the
/// whole-sample mean and standard deviation. Strongly enriched markers become
/// `Marker+` terms and strongly depleted markers become `Marker-` terms.
pub fn mem_cluster_names(
    labels: &[usize],
    values: &[f64],
    n_params: usize,
    param_names: &[String],
) -> Vec<MemClusterName> {
    let n_params = n_params.max(1);
    let events = labels.len().min(values.len() / n_params);
    if events == 0 {
        return Vec::new();
    }
    let mut global_sums = vec![0.0_f64; n_params];
    for e in 0..events {
        for c in 0..n_params {
            global_sums[c] += values[e * n_params + c];
        }
    }
    let global_means: Vec<f64> = global_sums.iter().map(|sum| sum / events as f64).collect();
    let mut global_vars = vec![0.0_f64; n_params];
    for e in 0..events {
        for c in 0..n_params {
            let delta = values[e * n_params + c] - global_means[c];
            global_vars[c] += delta * delta;
        }
    }
    let global_stds: Vec<f64> = global_vars
        .iter()
        .map(|var| (var / events as f64).sqrt())
        .collect();

    let mut order: Vec<usize> = Vec::new();
    let mut sums: Vec<Vec<f64>> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    for e in 0..events {
        let label = labels[e];
        let position = match order.iter().position(|cluster| *cluster == label) {
            Some(position) => position,
            None => {
                order.push(label);
                sums.push(vec![0.0; n_params]);
                counts.push(0);
                order.len() - 1
            }
        };
        for c in 0..n_params {
            sums[position][c] += values[e * n_params + c];
        }
        counts[position] += 1;
    }

    order
        .iter()
        .enumerate()
        .map(|(i, cluster)| {
            let enrichment: Vec<f64> = (0..n_params)
                .map(|c| {
                    if counts[i] == 0 || global_stds[c] <= 0.0 {
                        0.0
                    } else {
                        let mean = sums[i][c] / counts[i] as f64;
                        (mean - global_means[c]) / global_stds[c]
                    }
                })
                .collect();
            let marker = |c: usize| {
                param_names
                    .get(c)
                    .cloned()
                    .unwrap_or_else(|| format!("P{c}"))
            };
            let mut positive: Vec<(usize, f64)> = enrichment
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, z)| *z >= 1.0)
                .collect();
            positive.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut negative: Vec<(usize, f64)> = enrichment
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, z)| *z <= -1.0)
                .collect();
            negative.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut parts: Vec<String> = positive
                .iter()
                .take(3)
                .map(|(c, _)| format!("{}+", marker(*c)))
                .collect();
            parts.extend(
                negative
                    .iter()
                    .take(2)
                    .map(|(c, _)| format!("{}-", marker(*c))),
            );
            let name = if parts.is_empty() {
                format!("Cluster {cluster}")
            } else {
                parts.join(" ")
            };
            MemClusterName {
                cluster: *cluster,
                name,
                enrichment,
            }
        })
        .collect()
}

/// Calibration fit for one channel: standard intensity -> fluorescence
/// (A10 fluorescence calibration to MESF/absolute units).
#[derive(Debug, Clone, PartialEq)]
pub struct CalibrationFit {
    pub channel: usize,
    pub standards: Vec<(f64, f64)>,
    pub slope: f64,
    pub intercept: f64,
    pub residuals: Vec<f64>,
}

/// Fit a linear calibration from standard samples (standard intensity ->
/// fluorescence) (A10 fluorescence calibration to MESF/absolute units).
pub fn fit_calibration(standards: &[(f64, f64)]) -> Option<CalibrationFit> {
    if standards.len() < 2 {
        return None;
    }
    let n = standards.len() as f64;
    let sx = standards.iter().map(|s| s.0).sum::<f64>();
    let sy = standards.iter().map(|s| s.1).sum::<f64>();
    let sxx = standards.iter().map(|s| s.0 * s.0).sum::<f64>();
    let sxy = standards.iter().map(|s| s.0 * s.1).sum::<f64>();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    let residuals = standards
        .iter()
        .map(|s| s.1 - (slope * s.0 + intercept))
        .collect();
    Some(CalibrationFit {
        channel: 0,
        standards: standards.to_vec(),
        slope,
        intercept,
        residuals,
    })
}

/// Convert a measured fluorescence value to MESF via the calibration fit
/// (A10 fluorescence calibration to MESF/absolute units).
pub fn fluorescence_to_mesf(fluorescence: f64, fit: &CalibrationFit) -> f64 {
    (fluorescence - fit.intercept) / fit.slope
}

/// Embed model: linear mapping from a derived parameter value to a target
/// parameter value (A8 embed).
#[derive(Debug, Clone, PartialEq)]
pub struct EmbedModel {
    pub reference_sample: usize,
    pub target_parameter: String,
    pub slope: f64,
    pub intercept: f64,
}

/// Fit an embed model from (derived, target) standard pairs (A8 embed).
pub fn fit_embed_model(standards: &[(f64, f64)], target_parameter: &str) -> Option<EmbedModel> {
    if standards.len() < 2 {
        return None;
    }
    let n = standards.len() as f64;
    let sx = standards.iter().map(|s| s.0).sum::<f64>();
    let sy = standards.iter().map(|s| s.1).sum::<f64>();
    let sxx = standards.iter().map(|s| s.0 * s.0).sum::<f64>();
    let sxy = standards.iter().map(|s| s.0 * s.1).sum::<f64>();
    let denom = n * sxx - sx * sx;
    if denom.abs() < 1e-12 {
        return None;
    }
    let slope = (n * sxy - sx * sy) / denom;
    let intercept = (sy - slope * sx) / n;
    Some(EmbedModel {
        reference_sample: 0,
        target_parameter: target_parameter.to_string(),
        slope,
        intercept,
    })
}

/// Map a derived parameter value through the embed model (A8).
pub fn embed_value(value: f64, model: &EmbedModel) -> f64 {
    model.slope * value + model.intercept
}

/// Forward-map a batch of derived parameter values through the embed model
/// (A8): the mapping applied to a new sample's derived parameters.
pub fn embed_values(values: &[f64], model: &EmbedModel) -> Vec<f64> {
    values.iter().map(|v| embed_value(*v, model)).collect()
}

/// Automatic phase gates (G1, S, G2/M) derived from a cell-cycle fit (A1):
/// events with DNA below the G1/S boundary are G1, events between the G1/S and
/// S/G2M boundaries are S, and events above the S/G2M boundary are G2/M.
pub fn phase_gates(fit: &CellCycleFit, param_idx: usize) -> Vec<RangeGate> {
    let g1s = fit.phase_boundaries[0].clamp(0.0, f32::MAX as f64) as f32;
    let sg2m = fit.phase_boundaries[1].clamp(0.0, f32::MAX as f64) as f32;
    vec![
        RangeGate::new(param_idx, 0.0, g1s),
        RangeGate::new(param_idx, g1s, sg2m),
        RangeGate::new(param_idx, sg2m, f32::MAX),
    ]
}

#[test]
fn tsne_is_deterministic_for_fixed_seed() {
    let points = [
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 1.0],
        [8.0, 8.0],
        [9.0, 8.0],
        [8.0, 9.0],
    ];
    let a = tsne(&points, 4, 50, 7);
    let b = tsne(&points, 4, 50, 7);
    assert_eq!(a.embedding, b.embedding);
    assert_eq!(a.iterations, 50);
}

#[test]
fn tsne_handles_empty_and_single_points() {
    let empty = tsne(&[], 5, 50, 3);
    assert!(empty.embedding.is_empty());
    let single = tsne(&[[3.0, 4.0]], 5, 50, 3);
    assert_eq!(single.embedding.len(), 1);
    assert_eq!(single.embedding[0], [0.0, 0.0]);
}

#[test]
fn tsne_two_well_separated_blobs_separate_in_embedding() {
    let mut points = Vec::new();
    for i in 0..20 {
        points.push([0.0 + (i as f64) * 0.1, 0.0 + (i as f64) * 0.05]);
    }
    for i in 0..20 {
        points.push([50.0 + (i as f64) * 0.1, 50.0 + (i as f64) * 0.05]);
    }
    let emb = tsne(&points, 5, 300, 11);
    let mut sums = [[0.0_f64; 2]; 2];
    let mut counts = [0usize; 2];
    for (i, &p) in points.iter().enumerate() {
        let c = if p[0] < 25.0 { 0 } else { 1 };
        for (d, _) in (0..2).enumerate() {
            sums[c][d] += emb.embedding[i][d];
        }
        counts[c] += 1;
    }
    let c0 = [sums[0][0] / counts[0] as f64, sums[0][1] / counts[0] as f64];
    let c1 = [sums[1][0] / counts[1] as f64, sums[1][1] / counts[1] as f64];
    let dist = ((c0[0] - c1[0]).powi(2) + (c0[1] - c1[1]).powi(2)).sqrt();
    assert!(
        dist > 10.0,
        "embedding centroids should be separated, got {dist}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn profile_basic_stats() {
        let data = array![[1.0], [2.0], [3.0], [4.0], [5.0]];
        let indices = vec![0, 1, 2, 3, 4];
        let profile = compute_profile("All", &data, &indices, &["FSC".to_string()], None, Some(5));
        assert_eq!(profile.count, 5);
        let mean = profile
            .stats
            .iter()
            .find(|s| s.statistic == Statistic::Mean && s.parameter == "FSC")
            .unwrap()
            .value;
        assert!((mean - 3.0).abs() < 1e-6);
        let median = profile
            .stats
            .iter()
            .find(|s| s.statistic == Statistic::Median && s.parameter == "FSC")
            .unwrap()
            .value;
        assert!((median - 3.0).abs() < 1e-6);
        let pct = profile.percent_of_total().unwrap();
        assert!((pct - 100.0).abs() < 1e-6);
    }

    #[test]
    fn subset_percent() {
        let data = array![[1.0], [2.0], [10.0], [20.0]];
        let indices = vec![2, 3];
        let profile = compute_profile(
            "High",
            &data,
            &indices,
            &["FSC".to_string()],
            Some(4),
            Some(4),
        );
        assert_eq!(profile.count, 2);
        assert!((profile.percent_of_parent().unwrap() - 50.0).abs() < 1e-6);
        assert!((profile.percent_of_total().unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn freq_of_against_grandparent_and_total() {
        // 20 events, single parameter with values 0..=19.
        let data = ndarray::array![
            [0.0f32],
            [1.0],
            [2.0],
            [3.0],
            [4.0],
            [5.0],
            [6.0],
            [7.0],
            [8.0],
            [9.0],
            [10.0],
            [11.0],
            [12.0],
            [13.0],
            [14.0],
            [15.0],
            [16.0],
            [17.0],
            [18.0],
            [19.0]
        ];
        // Parent = events 0..=9 (10 events); child = events 10..=19 (10 events).
        let child_indices: Vec<usize> = (10..20).collect();
        let frequencies = vec![
            ("Grandparent".to_string(), Some(20usize)),
            ("Total".to_string(), Some(20usize)),
        ];
        let profile = compute_profile_with_denominators(
            "Child",
            &data,
            &child_indices,
            &["FSC".to_string()],
            Some(10),
            Some(20),
            &frequencies,
        );
        assert_eq!(profile.count, 10);
        // Against the immediate parent: 100%.
        let of_parent = profile
            .stats
            .iter()
            .find(|s| s.statistic == Statistic::PercentOfParent)
            .unwrap();
        assert!((of_parent.value - 100.0).abs() < 1e-6);
        // Against the grandparent (root): 50%.
        let of_gp = profile
            .stats
            .iter()
            .find(|s| {
                s.statistic
                    == Statistic::FreqOf {
                        denominator: "Grandparent".to_string(),
                    }
            })
            .unwrap();
        assert!((of_gp.value - 50.0).abs() < 1e-6);
        assert_eq!(of_gp.parameter, "%Grandparent");
        // Against the total: 50%.
        let of_total = profile
            .stats
            .iter()
            .find(|s| s.statistic == Statistic::PercentOfTotal)
            .unwrap();
        assert!((of_total.value - 50.0).abs() < 1e-6);
    }

    #[test]
    fn freq_of_against_custom_denominator() {
        let data = ndarray::array![[0.0f32], [1.0], [2.0], [3.0], [4.0], [5.0]];
        let indices = vec![2, 3]; // values 2.0 and 3.0 -> count 2
        let frequencies = vec![("CustomGroup".to_string(), Some(6usize))];
        let profile = compute_profile_with_denominators(
            "Subset",
            &data,
            &indices,
            &["FSC".to_string()],
            Some(6),
            Some(6),
            &frequencies,
        );
        let of_custom = profile
            .stats
            .iter()
            .find(|s| {
                s.statistic
                    == Statistic::FreqOf {
                        denominator: "CustomGroup".to_string(),
                    }
            })
            .unwrap();
        assert!((of_custom.value - (2.0 / 6.0 * 100.0)).abs() < 1e-6);
        assert_eq!(of_custom.parameter, "%CustomGroup");
    }

    #[test]
    fn freq_of_skips_missing_denominators() {
        let data = ndarray::array![[1.0], [2.0], [3.0]];
        let profile = compute_profile_with_denominators(
            "All",
            &data,
            &[0, 1, 2],
            &["FSC".to_string()],
            None,
            Some(3),
            &[
                ("Parent".to_string(), None),
                ("Grandparent".to_string(), Some(3)),
            ],
        );
        // Only the grandparent (total) frequency is present; no parent entry.
        assert!(
            profile
                .stats
                .iter()
                .all(|s| !matches!(s.statistic, Statistic::PercentOfParent))
        );
        let of_gp = profile
            .stats
            .iter()
            .find(|s| {
                s.statistic
                    == Statistic::FreqOf {
                        denominator: "Grandparent".to_string(),
                    }
            })
            .unwrap();
        assert!((of_gp.value - 100.0).abs() < 1e-6);
    }

    #[test]
    fn percentile_uses_linear_interpolation() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((percentile(&values, 0.0) - 1.0).abs() < 1e-12);
        assert!((percentile(&values, 50.0) - 3.0).abs() < 1e-12);
        assert!((percentile(&values, 25.0) - 2.0).abs() < 1e-12);
        assert!((percentile(&values, 75.0) - 4.0).abs() < 1e-12);
        assert!((percentile(&values, 100.0) - 5.0).abs() < 1e-12);
        // Interpolated between 1 and 2.
        assert!((percentile(&values, 12.5) - 1.5).abs() < 1e-12);
    }

    #[test]
    fn mode_finds_dominant_peak() {
        let mut values = vec![10.0; 100];
        values.extend([1.0, 2.0, 3.0, 100.0]);
        let m = mode(&values);
        assert!((m - 10.0).abs() < 1.0, "mode was {m}");
        assert!((mode(&[7.0, 7.0, 7.0]) - 7.0).abs() < 1e-12);
        assert!(mode(&[]).is_nan());
    }

    #[test]
    fn robust_statistics_and_cdf() {
        let values = [1.0, 2.0, 3.0, 4.0, 100.0];
        let median = 3.0;
        let robust = robust_sd_sorted(median, &values);
        // Deviations: 2, 1, 0, 1, 97 -> median 1 -> 1.4826.
        assert!((robust - 1.4826).abs() < 1e-6);
        assert!((cdf_at(&values, 3.0) - 0.6).abs() < 1e-12);
        assert!((cdf_at(&values, 0.0) - 0.0).abs() < 1e-12);
        assert!((cdf_at(&values, 1000.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn expanded_profile_contains_new_statistics() {
        let data = array![[1.0], [2.0], [2.0], [3.0], [4.0], [5.0]];
        let profile = compute_profile(
            "All",
            &data,
            &[0, 1, 2, 3, 4, 5],
            &["FSC".to_string()],
            None,
            Some(6),
        );
        let has = |s: &Statistic| profile.stats.iter().any(|p| &p.statistic == s);
        assert!(has(&Statistic::Mode));
        assert!(has(&Statistic::Percent5));
        assert!(has(&Statistic::Percent95));
        assert!(has(&Statistic::RobustSd));
        assert!(has(&Statistic::RobustCv));
        assert!(has(&Statistic::Cdf(2.5)));
    }

    #[test]
    fn statistic_set_presets_and_apply() {
        let data = array![[1.0], [2.0], [3.0], [4.0]];
        let profile = compute_profile(
            "All",
            &data,
            &[0, 1, 2, 3],
            &["FSC".to_string()],
            None,
            Some(4),
        );

        let basic = StatisticSet::basic();
        let applied = basic.apply(&profile);
        assert!(
            applied
                .iter()
                .any(|s| s.parameter == "FSC" && s.statistic == Statistic::Mean)
        );
        assert!(
            applied
                .iter()
                .any(|s| s.statistic == Statistic::PercentOfTotal)
        );
        // De-duplicated by (parameter, statistic).
        let means = applied
            .iter()
            .filter(|s| s.parameter == "FSC" && s.statistic == Statistic::Mean)
            .count();
        assert_eq!(means, 1);

        let robust = StatisticSet::robust();
        assert!(
            robust
                .apply(&profile)
                .iter()
                .any(|s| s.statistic == Statistic::RobustSd)
        );

        let full = StatisticSet::full();
        assert!(
            full.apply(&profile)
                .iter()
                .any(|s| s.statistic == Statistic::Percent10)
        );
        assert_eq!(StatisticSet::presets().len(), 3);
    }

    #[test]
    fn statistic_set_parameter_selector() {
        let data = array![[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]];
        let profile = compute_profile(
            "All",
            &data,
            &[0, 1, 2],
            &["FSC".to_string(), "SSC".to_string()],
            None,
            Some(3),
        );
        let set = StatisticSet::new(
            "FSC only",
            vec![StatSpec {
                parameter: Some("FSC".to_string()),
                statistic: Statistic::Mean,
                expected: None,
                heat: None,
            }],
        );
        let applied = set.apply(&profile);
        assert_eq!(applied.len(), 1);
        assert_eq!(applied[0].parameter, "FSC");
        assert_eq!(set.parameter_names(&profile), vec!["FSC".to_string()]);
    }

    #[test]
    fn expected_range_flags_out_of_range_values() {
        let set = StatisticSet::new(
            "E",
            vec![StatSpec {
                parameter: Some("CD4".to_string()),
                statistic: Statistic::Mean,
                expected: Some(ExpectedRange {
                    min: 0.0,
                    max: 10.0,
                    warn_only: false,
                }),
                heat: None,
            }],
        );
        assert!(set.in_range("CD4", &Statistic::Mean, 5.0));
        assert!(set.in_range("CD4", &Statistic::Mean, 10.0)); // inclusive bounds
        assert!(!set.in_range("CD4", &Statistic::Mean, 11.0));
        // Without an expectation every value is in range.
        assert!(set.in_range("FSC", &Statistic::Mean, 3.0));
        // warn_only only affects reporting, not the in-range decision.
        let set = StatisticSet::new(
            "E2",
            vec![StatSpec {
                parameter: Some("CD4".to_string()),
                statistic: Statistic::Mean,
                expected: Some(ExpectedRange {
                    min: 0.0,
                    max: 10.0,
                    warn_only: true,
                }),
                heat: None,
            }],
        );
        assert!(set.in_range("CD4", &Statistic::Mean, 5.0));
        assert!(!set.in_range("CD4", &Statistic::Mean, 11.0));
    }

    #[test]
    fn heat_range_normalises_values() {
        let set = StatisticSet::new(
            "H",
            vec![StatSpec {
                parameter: Some("CD4".to_string()),
                statistic: Statistic::Median,
                expected: None,
                heat: Some(HeatRange {
                    min: 0.0,
                    max: 100.0,
                    palette: "viridis".to_string(),
                }),
            }],
        );
        assert_eq!(
            set.heat_position("CD4", &Statistic::Median, 50.0),
            Some(0.5)
        );
        assert_eq!(
            set.heat_position("CD4", &Statistic::Median, 150.0),
            Some(1.0)
        );
        assert_eq!(
            set.heat_position("CD4", &Statistic::Median, -10.0),
            Some(0.0)
        );
        // A degenerate heat range clamps to zero.
        let set = StatisticSet::new(
            "H2",
            vec![StatSpec {
                parameter: Some("CD4".to_string()),
                statistic: Statistic::Median,
                expected: None,
                heat: Some(HeatRange {
                    min: 5.0,
                    max: 5.0,
                    palette: "viridis".to_string(),
                }),
            }],
        );
        assert_eq!(
            set.heat_position("CD4", &Statistic::Median, 99.0),
            Some(0.0)
        );
    }

    #[test]
    fn expected_and_heat_ranges_round_trip_through_serde() {
        let range = ExpectedRange {
            min: 0.0,
            max: 12.0,
            warn_only: true,
        };
        let json = serde_json::to_string(&range).unwrap();
        let back: ExpectedRange = serde_json::from_str(&json).unwrap();
        assert_eq!(back, range);

        let heat = HeatRange {
            min: 0.0,
            max: 1.0,
            palette: "plasma".to_string(),
        };
        let json = serde_json::to_string(&heat).unwrap();
        let back: HeatRange = serde_json::from_str(&json).unwrap();
        assert_eq!(back, heat);
    }

    #[test]
    fn ks_statistic_is_zero_for_identical_samples() {
        let a = [1.0_f64, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(ks_statistic(&a, &a), 0.0);
        let b = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(ks_statistic(&a, &b), ks_statistic(&b, &a));
    }

    #[test]
    fn ks_statistic_is_positive_for_shifted_samples() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert!(ks_statistic(&a, &b) > 0.0);
        assert!(ks_statistic(&[], &[1.0]).abs() < 1e-9);
    }

    #[test]
    fn chi_squared_statistic_sums_squared_differences() {
        // (2-1)^2/1 + (2-3)^2/3 = 1 + 0.333...
        let stat = chi_squared_statistic(&[2.0, 2.0], &[1.0, 3.0]);
        assert!((stat - 1.3333).abs() < 1e-3);
    }

    #[test]
    fn standard_error_matches_closed_form() {
        let se = standard_error(0.25, 100);
        assert!((se - (0.001875_f64).sqrt()).abs() < 1e-9);
        assert_eq!(standard_error(0.5, 0), 0.0);
    }

    #[test]
    fn precision_recall_f1_perfect_and_partial() {
        let perfect = precision_recall_f1(&[true, true, false], &[true, true, false]);
        assert!((perfect.0 - 1.0).abs() < 1e-9);
        assert!((perfect.1 - 1.0).abs() < 1e-9);
        assert!((perfect.2 - 1.0).abs() < 1e-9);
        let partial = precision_recall_f1(&[true, true, false], &[true, false, false]);
        assert!((partial.0 - 1.0).abs() < 1e-9); // precision: no false positives
        assert!((partial.1 - 0.5).abs() < 1e-9); // recall: one of two positives caught
        assert!((partial.2 - (2.0 * 1.0 * 0.5 / 1.5)).abs() < 1e-9);
    }

    #[test]
    fn match_ratio_of_identical_and_disjoint_sets() {
        let same = vec![true, true, false, true];
        assert_eq!(match_ratio(&same, &same), 1.0);
        let disjoint = vec![false, false, false, false];
        assert_eq!(match_ratio(&same, &disjoint), 0.0);
        let partial = vec![true, false, false, false];
        assert!((match_ratio(&same, &partial) - 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn compare_populations_detects_separation() {
        let a = [1.0, 1.0, 2.0, 2.0, 3.0];
        let b = [10.0, 20.0, 30.0, 40.0, 50.0];
        let cmp = compare_populations(&a, &b, 4);
        assert_eq!(cmp.n_a, 5);
        assert_eq!(cmp.n_b, 5);
        assert!(cmp.ks_statistic > 0.0);
        assert!(cmp.chi_squared_statistic > 0.0);
        let same = compare_populations(&a, &a, 4);
        assert_eq!(same.ks_statistic, 0.0);
        assert_eq!(same.chi_squared_statistic, 0.0);
    }

    #[test]
    fn moving_average_smooths_with_trailing_window() {
        assert_eq!(
            smooth_moving_average(&[1.0, 2.0, 3.0, 4.0], 2),
            vec![1.0, 1.5, 2.5, 3.5]
        );
    }

    #[test]
    fn gaussian_smoothing_preserves_constant_series() {
        assert_eq!(smooth_gaussian(&[5.0, 5.0, 5.0], 1.0), vec![5.0, 5.0, 5.0]);
    }

    #[test]
    fn peak_time_returns_time_of_maximum() {
        let times = [0.0, 1.0, 2.0, 3.0, 4.0];
        let values = [1.0, 3.0, 2.0, 5.0, 1.0];
        assert_eq!(peak_time(&times, &values), Some(3.0));
        assert_eq!(peak_time(&[], &[1.0]), None);
    }

    #[test]
    fn threshold_crossing_finds_first_crossing() {
        let times = [0.0, 1.0, 2.0, 3.0];
        let values = [0.1, 0.2, 0.5, 0.9];
        assert_eq!(threshold_crossing(&times, &values, 0.4), Some(2.0));
        assert_eq!(threshold_crossing(&times, &values, 5.0), None);
    }

    #[test]
    fn auc_uses_trapezoidal_rule() {
        let times = [0.0, 1.0, 2.0];
        let values = [1.0, 2.0, 3.0];
        assert!((auc(&times, &values, 0.0, 2.0) - 4.0).abs() < 1e-9);
        assert!((auc(&times, &values, 0.0, 1.0) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn kinetic_stats_report_peak_and_area() {
        let times = [0.0, 1.0, 2.0, 3.0, 4.0];
        let values = [1.0, 3.0, 2.0, 5.0, 1.0];
        let ks = kinetic_stats(&times, &values);
        assert!((ks.peak_time - 3.0).abs() < 1e-9);
        assert!((ks.peak_value - 5.0).abs() < 1e-9);
        assert!((ks.auc - 11.0).abs() < 1e-9);
        assert!(ks.rise_slope.is_finite());
        assert!(ks.decay_slope.is_finite());
    }

    #[test]
    fn population_difference_is_zero_for_identical_distributions() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let d = population_difference(&a, &a);
        assert_eq!(d.n_a, 5);
        assert_eq!(d.n_b, 5);
        assert_eq!(d.cdf_differences.len(), 5);
        assert!(d.cdf_differences.iter().all(|(_, d)| *d == 0.0));
        assert_eq!(d.max_abs_cdf_difference, 0.0);
    }

    #[test]
    fn population_difference_detects_shifted_distributions() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        let b = [10.0, 20.0, 30.0, 40.0, 50.0];
        let d = population_difference(&a, &b);
        assert!(d.max_abs_cdf_difference > 0.0);
        let peak = d
            .cdf_differences
            .iter()
            .max_by(|x, y| x.1.abs().partial_cmp(&y.1.abs()).unwrap())
            .unwrap();
        assert!((peak.1.abs() - d.max_abs_cdf_difference).abs() < 1e-9);
    }

    #[test]
    fn overton_statistic_matches_ks_comparison() {
        let a = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(overton_statistic(&a, &a), 0.0);
        let b = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert!((overton_statistic(&a, &b) - ks_statistic(&a, &b)).abs() < 1e-12);
    }

    #[test]
    fn analyze_proliferation_computes_generation_peaks_and_index() {
        let a = analyze_proliferation(&[10.0, 20.0, 30.0, 40.0, 50.0]);
        assert_eq!(a.generation_peaks, vec![10.0, 20.0, 30.0, 40.0, 50.0]);
        assert!((a.proliferation_index - 5.0).abs() < 1e-9);
    }

    #[test]
    fn analyze_proliferation_single_generation_has_unit_index() {
        let a = analyze_proliferation(&[100.0]);
        assert_eq!(a.generation_peaks, vec![100.0]);
        assert!((a.proliferation_index - 1.0).abs() < 1e-9);
    }

    #[test]
    fn analyze_proliferation_empty_trajectory_has_no_index() {
        let a = analyze_proliferation(&[]);
        assert!(a.generation_peaks.is_empty());
        assert!((a.proliferation_index - 0.0).abs() < 1e-9);
    }

    #[test]
    fn fit_proliferation_recovers_exponential_growth() {
        let fit = fit_proliferation(&[10.0, 20.0, 40.0, 80.0, 160.0]).unwrap();
        assert!((fit.growth_rate - 2.0).abs() < 1e-6);
        assert!((fit.r_squared - 1.0).abs() < 1e-6);
    }

    #[test]
    fn fit_proliferation_rejects_degenerate_trajectories() {
        assert!(fit_proliferation(&[]).is_none());
        assert!(fit_proliferation(&[100.0]).is_none());
        assert!(fit_proliferation(&[0.0, 5.0]).is_none());
    }

    #[test]
    fn analyze_proliferation_reports_the_fit() {
        let a = analyze_proliferation(&[10.0, 20.0, 40.0, 80.0, 160.0]);
        let fit = a.fit.expect("two positive generations should yield a fit");
        assert!((fit.growth_rate - 2.0).abs() < 1e-6);
        assert!((fit.r_squared - 1.0).abs() < 1e-6);
    }

    #[test]
    fn generation_gates_derive_windows_from_proliferation_peaks() {
        let analysis = analyze_proliferation(&[10.0, 20.0, 40.0, 80.0, 160.0]);
        let gates = generation_gates(&analysis, 3);
        assert_eq!(gates.len(), 4);
        assert_eq!((gates[0].low, gates[0].high), (10.0, 20.0));
        assert_eq!((gates[1].low, gates[1].high), (20.0, 40.0));
        assert_eq!((gates[2].low, gates[2].high), (40.0, 80.0));
        assert_eq!((gates[3].low, gates[3].high), (80.0, 160.0));
    }

    #[test]
    fn generation_gates_empty_without_two_or_more_peaks() {
        let single = analyze_proliferation(&[100.0]);
        assert!(generation_gates(&single, 2).is_empty());
        let empty = analyze_proliferation(&[]);
        assert!(generation_gates(&empty, 2).is_empty());
    }

    #[test]
    fn phase_gates_split_population_into_three_phases() {
        let fit = CellCycleFit {
            means: [100.0, 450.0, 550.0],
            sigmas: [10.0, 60.0, 70.0],
            proportions: [0.4, 0.3, 0.3],
            log_likelihood: 0.0,
            phase_percentages: [40.0, 30.0, 30.0],
            phase_boundaries: [120.0, 570.0],
        };
        let gates = phase_gates(&fit, 3);
        assert_eq!(gates.len(), 3);
        assert_eq!(gates[0].param_idx, 3);
        assert_eq!(gates[0].low, 0.0);
        assert_eq!(gates[0].high, 120.0);
        assert_eq!(gates[1].low, 120.0);
        assert_eq!(gates[1].high, 570.0);
        assert_eq!(gates[2].low, 570.0);
        assert_eq!(gates[2].high, f32::MAX);
    }

    #[test]
    fn phase_gates_use_derived_boundaries_from_fit() {
        let fit = fit_cell_cycle(&[
            100.0, 110.0, 120.0, 130.0, 140.0, 150.0, 460.0, 470.0, 480.0, 490.0, 560.0, 570.0,
        ]);
        let gates = phase_gates(&fit, 0);
        assert_eq!(gates.len(), 3);
        assert!(fit.phase_boundaries[0] > 0.0);
        assert!(fit.phase_boundaries[0] < fit.phase_boundaries[1]);
        // G1 is bounded above by the G1/S boundary; S spans it; G2/M starts at the S/G2M boundary.
        assert_eq!(gates[0].high, fit.phase_boundaries[0] as f32);
        assert_eq!(gates[1].low, fit.phase_boundaries[0] as f32);
        assert_eq!(gates[1].high, fit.phase_boundaries[1] as f32);
        assert_eq!(gates[2].low, fit.phase_boundaries[1] as f32);
    }

    fn stat(parameter: &str, statistic: Statistic, value: f64) -> PopulationStat {
        PopulationStat {
            parameter: parameter.to_string(),
            statistic,
            value,
        }
    }

    #[test]
    fn difference_table_is_zero_for_identical_profiles() {
        let test = PopulationProfile {
            name: "test".into(),
            count: 10,
            ..Default::default()
        };
        let control = PopulationProfile {
            name: "control".into(),
            count: 10,
            parent_count: None,
            total_count: None,
            stats: vec![],
        };
        let rows = difference_table(&test, &control);
        assert!(rows.is_empty()); // no statistics computed yet -> no rows
    }

    #[test]
    fn difference_table_computes_test_minus_control() {
        let test = PopulationProfile {
            name: "test".into(),
            count: 20,
            parent_count: None,
            total_count: None,
            stats: vec![stat("FITC", Statistic::Mean, 10.0)],
        };
        let control = PopulationProfile {
            name: "control".into(),
            count: 20,
            parent_count: None,
            total_count: None,
            stats: vec![stat("FITC", Statistic::Mean, 5.0)],
        };
        let rows = difference_table(&test, &control);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].parameter, "FITC");
        assert_eq!(rows[0].statistic, Statistic::Mean);
        assert_eq!(rows[0].difference, 5.0);
        assert_eq!(rows[0].test_value, 10.0);
        assert_eq!(rows[0].control_value, 5.0);
    }

    #[test]
    fn difference_table_skips_statistics_missing_from_control() {
        let test = PopulationProfile {
            name: "test".into(),
            count: 20,
            parent_count: None,
            total_count: None,
            stats: vec![
                stat("FITC", Statistic::Mean, 10.0),
                stat("PE", Statistic::Mean, 3.0),
            ],
        };
        let control = PopulationProfile {
            name: "control".into(),
            count: 20,
            parent_count: None,
            total_count: None,
            stats: vec![stat("FITC", Statistic::Mean, 5.0)],
        };
        let rows = difference_table(&test, &control);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].parameter, "FITC");
    }

    #[test]
    fn cell_cycle_fit_recovers_synthetic_mixture() {
        let mut state = 12345u64;
        let mut rand = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 33) as f64) / ((1u64 << 31) as f64)
        };
        let components = [
            (100.0_f64, 10.0, 0.4),
            (450.0, 60.0, 0.3),
            (550.0, 70.0, 0.3),
        ];
        let mut values: Vec<f64> = Vec::new();
        for _ in 0..200 {
            let r = rand();
            let mut acc = 0.0;
            let mut comp = 2;
            for (k, &(_mu, _sigma, prop)) in components.iter().enumerate() {
                acc += prop;
                if r <= acc {
                    comp = k;
                    break;
                }
            }
            let s = if comp == 0 {
                10.0
            } else if comp == 1 {
                60.0
            } else {
                70.0
            };
            let mu = components[comp].0;
            let u1 = rand().max(1e-12);
            let u2 = rand();
            let z = (-2.0 * u1.ln()).sqrt() * (-2.0 * (u2 * std::f64::consts::PI)).sin();
            values.push(mu + z * s);
        }
        let fit = fit_cell_cycle(&values);
        eprintln!(
            "means={:?} sigmas={:?} props={:?} ll={}",
            fit.means, fit.sigmas, fit.proportions, fit.log_likelihood
        );
        assert!(fit.log_likelihood.is_finite());
        for (k, component) in components.iter().enumerate() {
            assert!(
                (fit.proportions[k] - component.2).abs() < 0.12,
                "proportion {k} = {}",
                fit.proportions[k]
            );
        }
        for (k, component) in components.iter().enumerate() {
            assert!(
                (fit.phase_percentages[k] - component.2 * 100.0).abs() < 8.0,
                "phase percentage {k} = {}",
                fit.phase_percentages[k]
            );
        }
        assert!(fit.means[0] < fit.means[1]);
        assert!(fit.means[1] < fit.means[2]);
    }

    #[test]
    fn cell_cycle_fit_handles_empty_input() {
        let fit = fit_cell_cycle(&[]);
        assert_eq!(fit.phase_percentages.len(), 3);
    }

    #[test]
    fn kmeans_recovers_separated_clusters() {
        // Three well-separated groups: {0,1,2}, {30,31,32}, {60,61,62}.
        let values = [0.0, 1.0, 2.0, 30.0, 31.0, 32.0, 60.0, 61.0, 62.0];
        let (labels, centroids, _iterations) = kmeans(&values, 3, 7, 50);
        assert_eq!(labels.len(), values.len());
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[1], labels[2]);
        assert_eq!(labels[3], labels[4]);
        assert_eq!(labels[4], labels[5]);
        assert_eq!(labels[6], labels[7]);
        assert_eq!(labels[7], labels[8]);
        assert_ne!(labels[0], labels[3]);
        assert_ne!(labels[0], labels[6]);
        assert_ne!(labels[3], labels[6]);
        assert_eq!(centroids.len(), 3);
    }

    #[test]
    fn kmeans_is_deterministic_for_fixed_seed() {
        let values = [0.0, 0.0, 1.0, 10.0, 10.0, 11.0, 20.0, 20.0, 21.0];
        let a = kmeans(&values, 3, 7, 50);
        let b = kmeans(&values, 3, 7, 50);
        assert_eq!(a, b);
    }

    #[test]
    fn kmeans_single_cluster_assigns_every_point() {
        let values = [1.0, 2.0, 3.0];
        let (labels, centroids, _) = kmeans(&values, 1, 3, 10);
        assert_eq!(labels, vec![0, 0, 0]);
        assert_eq!(centroids.len(), 1);
    }

    #[test]
    fn sample_quality_flags_empty_and_saturated_samples() {
        let empty = compute_sample_quality(&[], 1, None, 5.0);
        assert_eq!(empty.event_count, 0);
        assert!(empty.anomalies.iter().any(|a| a == "no_events"));

        let values = [1.0, 10.0, 10.0];
        let report = compute_sample_quality(&values, 1, None, 5.0);
        assert_eq!(report.event_count, 3);
        assert!((report.saturation_fraction - 2.0 / 3.0).abs() < 1e-9);
        assert!(report.anomalies.iter().any(|a| a == "high_saturation"));
    }

    #[test]
    fn cluster_profiles_group_events_by_label() {
        // Events: (label, value) pairs -> values [1, 1, 5, 5, 9, 9], 1 param.
        let labels = [0usize, 0, 1, 1, 2, 2];
        let values = [1.0, 1.0, 5.0, 5.0, 9.0, 9.0];
        let profiles = cluster_profiles(&labels, &values, 1);
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].cluster, 0);
        assert_eq!(profiles[0].size, 2);
        assert_eq!(profiles[0].means, vec![1.0]);
        assert_eq!(profiles[1].cluster, 1);
        assert_eq!(profiles[1].size, 2);
        assert_eq!(profiles[1].means, vec![5.0]);
        assert_eq!(profiles[2].cluster, 2);
        assert_eq!(profiles[2].size, 2);
        assert_eq!(profiles[2].means, vec![9.0]);
    }

    #[test]
    fn cluster_comparison_detects_different_means() {
        // Clusters: [1, 2] and [5, 6] -> means 1.5 and 5.5, t should be large.
        let labels = [0usize, 0, 1, 1, 2, 2];
        let values = [1.0, 2.0, 5.0, 6.0, 9.0, 10.0];
        let cmp = compare_clusters(&labels, &values, 1, 0, 1);
        assert_eq!(cmp.cluster_a, 0);
        assert_eq!(cmp.cluster_b, 1);
        assert_eq!(cmp.channels.len(), 1);
        assert!(cmp.channels[0].t.abs() > 1.0, "t={}", cmp.channels[0].t);
    }

    #[test]
    fn cluster_comparison_identical_clusters_have_zero_t() {
        let labels = [0usize, 0, 1, 1];
        let values = [5.0, 5.0, 5.0, 5.0];
        let cmp = compare_clusters(&labels, &values, 1, 0, 1);
        assert_eq!(cmp.channels.len(), 1);
        assert_eq!(cmp.channels[0].t, 0.0);
    }

    #[test]
    fn cluster_results_csv_renders_profiles() {
        let labels = [0usize, 0, 1, 1, 2, 2];
        let values = [1.0, 1.0, 5.0, 5.0, 9.0, 9.0];
        let csv = cluster_results_csv(&labels, &values, 1);
        assert!(csv.starts_with("cluster,size,mean_0"));
        assert!(csv.contains("0,2,1"));
        assert!(csv.contains("1,2,5"));
        assert!(csv.contains("2,2,9"));
    }

    #[test]
    fn cluster_explorer_merges_cleans_and_reports_sizes() {
        let labels = [0usize, 0, 1, 1, 2, 2];
        let values = [1.0, 1.0, 5.0, 5.0, 9.0, 9.0];
        let mut explorer = ClusterExplorer::new(&labels, &values, 1);
        explorer.merge(&[2], 1);
        assert_eq!(explorer.labels, vec![0, 0, 1, 1, 1, 1]);
        assert_eq!(explorer.sizes(), vec![(0, 2), (1, 4)]);

        // Cluster 2 (mean 100) is nearest cluster 1 (mean 50), not cluster 0
        // (mean 1), so cleaning folds it into cluster 1.
        let labels = [0usize, 0, 0, 1, 1, 1, 2, 2];
        let values = [1.0, 1.0, 1.0, 50.0, 50.0, 50.0, 100.0, 100.0];
        let mut explorer = ClusterExplorer::new(&labels, &values, 1);
        explorer.clean(3);
        assert_eq!(explorer.sizes(), vec![(0, 3), (1, 5)]);
        assert_eq!(explorer.labels, vec![0, 0, 0, 1, 1, 1, 1, 1]);
    }

    #[test]
    fn cluster_heat_map_normalizes_channel_means() {
        let profiles = vec![
            ClusterProfile {
                cluster: 0,
                size: 2,
                means: vec![0.0, 10.0],
            },
            ClusterProfile {
                cluster: 1,
                size: 3,
                means: vec![5.0, 20.0],
            },
            ClusterProfile {
                cluster: 2,
                size: 4,
                means: vec![10.0, 30.0],
            },
        ];
        let heat = cluster_heat_map(&profiles);
        assert_eq!(heat, vec![vec![0.0, 0.0], vec![0.5, 0.5], vec![1.0, 1.0]]);
    }

    #[test]
    fn cluster_gates_cover_each_cluster_range() {
        let labels = [0usize, 0, 1, 1];
        let values = [1.0, 3.0, 10.0, 14.0];
        let gates = cluster_gates(&labels, &values, 1);
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].cluster, 0);
        assert_eq!(gates[0].gates[0].low, 1.0);
        assert_eq!(gates[0].gates[0].high, 3.0);
        assert_eq!(gates[1].cluster, 1);
        assert_eq!(gates[1].gates[0].low, 10.0);
        assert_eq!(gates[1].gates[0].high, 14.0);
    }

    #[test]
    fn cluster_results_clr_and_csv_round_trip() {
        let labels = [0usize, 0, 1, 1, 2, 2];
        let values = [1.0, 1.0, 5.0, 5.0, 9.0, 9.0];
        let profiles = cluster_profiles(&labels, &values, 1);

        let clr = cluster_results_clr(&labels, &values, 1);
        assert!(clr.starts_with("#CLR1\n"));
        assert!(clr.contains("cluster\tsize\tmean_0"));
        assert_eq!(parse_cluster_results_clr(&clr).unwrap(), profiles);

        let csv = cluster_results_csv(&labels, &values, 1);
        assert_eq!(parse_cluster_results_csv(&csv).unwrap(), profiles);

        let explorer = ClusterExplorer::from_results(&clr).unwrap();
        // Aggregated profiles cannot reconstruct event counts; the import
        // keeps one stand-in event per profile.
        assert_eq!(explorer.sizes(), vec![(0, 1), (1, 1), (2, 1)]);
        let imported = explorer.profiles();
        assert_eq!(imported.len(), profiles.len());
        for (left, right) in imported.iter().zip(profiles.iter()) {
            assert_eq!(left.cluster, right.cluster);
            assert_eq!(left.means, right.means);
        }
    }

    #[test]
    fn cluster_explorer_compare_and_heat_map_views() {
        let labels = [0usize, 0, 1, 1];
        let values = [1.0, 2.0, 9.0, 10.0];
        let explorer = ClusterExplorer::new(&labels, &values, 1);
        let comparison = explorer.compare(0, 1);
        assert_eq!(comparison.channels.len(), 1);
        assert!(comparison.channels[0].t.abs() > 1.0);
        let heat = explorer.heat_map();
        assert_eq!(heat.len(), 2);
        assert_eq!(heat[0], vec![0.0]);
        assert_eq!(heat[1], vec![1.0]);
        assert_eq!(explorer.gates().len(), 2);
    }

    #[test]
    fn hyperfinder_finds_bimodal_threshold() {
        let mut values = vec![1.0; 10];
        values.extend(vec![10.0; 10]);
        let gates = hyperfinder(&values, 1, 2);
        assert_eq!(gates.len(), 1);
        let gate = &gates[0];
        assert_eq!(gate.param_idx, 0);
        assert!((gate.threshold - 5.5).abs() < 1e-9);
        assert_eq!(gate.positive_count, 10);
        assert_eq!(gate.negative_count, 10);
        assert!((gate.score - 1.0).abs() < 1e-9);
        assert_eq!(gate.gate.low, 5.5);
        assert_eq!(gate.gate.high, 10.0);
    }

    #[test]
    fn hyperfinder_ranks_best_channel_and_respects_min_events() {
        // Channel 0 separates cleanly; channel 1 is a continuous ramp.
        let mut values = Vec::new();
        for i in 0..10 {
            values.push(1.0);
            values.push(i as f64);
        }
        for i in 0..10 {
            values.push(10.0);
            values.push((i + 10) as f64);
        }
        let gates = hyperfinder(&values, 2, 5);
        assert_eq!(gates.len(), 2);
        assert_eq!(gates[0].param_idx, 0);
        assert!(gates[0].score >= gates[1].score);

        // Requiring more events than each side has yields no gate.
        assert!(hyperfinder(&values, 2, 11).is_empty());
    }

    #[test]
    fn discover_gating_strategy_truncates_to_max_gates() {
        let mut values = vec![1.0; 10];
        values.extend(vec![10.0; 10]);
        let strategy = discover_gating_strategy(&values, 1, 2, 1);
        assert_eq!(strategy.gates.len(), 1);
        let empty = discover_gating_strategy(&values, 1, 2, 0);
        assert!(empty.gates.is_empty());
    }

    #[test]
    fn mem_cluster_names_mark_enriched_and_depleted_channels() {
        let labels = [0usize, 0, 1, 1];
        // Two channels per event: cluster 0 is CD3+ CD19-, cluster 1 is the
        // reverse.
        let values = [10.0, 0.0, 10.0, 0.0, 0.0, 10.0, 0.0, 10.0];
        let names = vec!["CD3".to_string(), "CD19".to_string()];
        let named = mem_cluster_names(&labels, &values, 2, &names);
        assert_eq!(named.len(), 2);
        assert_eq!(named[0].cluster, 0);
        assert_eq!(named[0].name, "CD3+ CD19-");
        assert!(named[0].enrichment[0] > 0.0);
        assert!(named[0].enrichment[1] < 0.0);
        assert_eq!(named[1].cluster, 1);
        assert_eq!(named[1].name, "CD19+ CD3-");
    }

    #[test]
    fn mem_cluster_names_fall_back_to_cluster_number() {
        let labels = [0usize, 0, 1, 1];
        let values = [5.0, 5.0, 5.0, 5.0];
        let names = vec!["CD3".to_string()];
        let named = mem_cluster_names(&labels, &values, 1, &names);
        assert_eq!(named[0].name, "Cluster 0");
        assert_eq!(named[1].name, "Cluster 1");
        assert_eq!(named[0].enrichment, vec![0.0]);
    }

    #[test]
    fn calibration_fit_recovers_perfect_linear_calibration() {
        let fit = fit_calibration(&[(1.0, 10.0), (2.0, 20.0), (3.0, 30.0)]).unwrap();
        assert!((fit.slope - 10.0).abs() < 1e-9);
        assert!((fit.intercept).abs() < 1e-9);
        assert_eq!(fit.residuals.len(), 3);
        for r in &fit.residuals {
            assert!((*r).abs() < 1e-9);
        }
        assert!((fluorescence_to_mesf(20.0, &fit) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn embed_maps_derived_value_through_linear_model() {
        let model = fit_embed_model(&[(1.0, 2.0), (2.0, 4.0), (3.0, 6.0)], "target").unwrap();
        assert!((model.slope - 2.0).abs() < 1e-9);
        assert!((model.intercept).abs() < 1e-9);
        assert!((embed_value(2.5, &model) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn embed_values_forward_maps_new_samples_derived_values() {
        let model = fit_embed_model(&[(1.0, 2.0), (2.0, 4.0), (3.0, 6.0)], "target").unwrap();
        let mapped = embed_values(&[2.5, 0.0, 7.5], &model);
        assert_eq!(mapped.len(), 3);
        assert!((mapped[0] - 5.0).abs() < 1e-9);
        assert!((mapped[1] - 0.0).abs() < 1e-9);
        assert!((mapped[2] - 15.0).abs() < 1e-9);
    }
}

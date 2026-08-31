use crate::types::{
    ForecastBundle, TimeSeriesData, TimeSeriesDescriptor, TimeSeriesPoint, TimeSeriesProjection,
    TimeSeriesRequest, TimeSeriesWindow, TimelineMarker,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::RwLock;
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimeSeriesError {
    #[error("invalid RFC3339 timestamp in {0}")]
    InvalidTimestamp(&'static str),
    #[error("time-series window must be non-empty")]
    EmptyWindow,
    #[error("maxPoints must be between 2 and 5000")]
    InvalidPointBudget,
    #[error("requested context does not match this operator account")]
    ContextMismatch,
    #[error("time-series repository lock was poisoned")]
    Poisoned,
    #[error("unsupported time-series projection schema")]
    UnsupportedProjectionSchema,
    #[error("time-series projection identity and sequence must be non-empty")]
    InvalidProjectionIdentity,
    #[error("time-series projection contains duplicate or unknown series")]
    InvalidProjectionSeries,
    #[error("time-series projection is not newer than the committed projection")]
    StaleProjection,
    #[error("time-series point values must be finite and valid for their render type")]
    InvalidPointValue,
    #[error("time-series projection exceeds its bounded point budget")]
    ProjectionTooLarge,
}

pub trait TimeSeriesPort: Send + Sync {
    fn catalog(&self) -> Result<Vec<TimeSeriesDescriptor>, TimeSeriesError>;
    fn forecast_bundles(&self) -> Result<Vec<ForecastBundle>, TimeSeriesError>;
    fn query(&self, request: &TimeSeriesRequest) -> Result<TimeSeriesWindow, TimeSeriesError>;
}

#[derive(Debug, Default)]
struct RepositoryState {
    sequence: u64,
    series: HashMap<String, TimeSeriesData>,
    markers: Vec<TimelineMarker>,
    projection_sequences: HashMap<String, u64>,
}

#[derive(Debug)]
pub struct InMemoryTimeSeriesPort {
    account_id: String,
    descriptors: BTreeMap<String, TimeSeriesDescriptor>,
    bundles: Vec<ForecastBundle>,
    state: RwLock<RepositoryState>,
}

impl InMemoryTimeSeriesPort {
    pub fn new(
        account_id: impl Into<String>,
        descriptors: Vec<TimeSeriesDescriptor>,
        bundles: Vec<ForecastBundle>,
    ) -> Self {
        Self {
            account_id: account_id.into(),
            descriptors: descriptors
                .into_iter()
                .map(|descriptor| (descriptor.id.clone(), descriptor))
                .collect(),
            bundles,
            state: RwLock::new(RepositoryState::default()),
        }
    }

    pub fn replace_series(
        &self,
        id: &str,
        mut points: Vec<TimeSeriesPoint>,
    ) -> Result<u64, TimeSeriesError> {
        let descriptor = self
            .descriptors
            .get(id)
            .cloned()
            .ok_or(TimeSeriesError::ContextMismatch)?;
        validate_and_sort_points(&mut points, None)?;
        let mut state = self.state.write().map_err(|_| TimeSeriesError::Poisoned)?;
        state.sequence = state.sequence.saturating_add(1);
        state
            .series
            .insert(id.to_owned(), TimeSeriesData { descriptor, points });
        Ok(state.sequence)
    }

    pub fn replace_markers(
        &self,
        mut markers: Vec<TimelineMarker>,
    ) -> Result<u64, TimeSeriesError> {
        validate_and_sort_markers(&mut markers)?;
        let mut state = self.state.write().map_err(|_| TimeSeriesError::Poisoned)?;
        state.sequence = state.sequence.saturating_add(1);
        state.markers = markers;
        Ok(state.sequence)
    }

    pub fn append_point(
        &self,
        id: &str,
        point: TimeSeriesPoint,
        max_retained_points: usize,
    ) -> Result<u64, TimeSeriesError> {
        if max_retained_points == 0 || max_retained_points > 1_000_000 {
            return Err(TimeSeriesError::InvalidPointBudget);
        }
        let descriptor = self
            .descriptors
            .get(id)
            .cloned()
            .ok_or(TimeSeriesError::ContextMismatch)?;
        validate_point(&point, None)?;
        let mut state = self.state.write().map_err(|_| TimeSeriesError::Poisoned)?;
        let data = state
            .series
            .entry(id.to_owned())
            .or_insert_with(|| TimeSeriesData {
                descriptor,
                points: Vec::new(),
            });
        match data
            .points
            .binary_search_by(|existing| existing.timestamp().cmp(point.timestamp()))
        {
            Ok(index) => data.points[index] = point,
            Err(index) => data.points.insert(index, point),
        }
        if data.points.len() > max_retained_points {
            let excess = data.points.len() - max_retained_points;
            data.points.drain(0..excess);
        }
        state.sequence = state.sequence.saturating_add(1);
        Ok(state.sequence)
    }

    pub fn replace_projection(
        &self,
        projection: TimeSeriesProjection,
    ) -> Result<u64, TimeSeriesError> {
        if projection.schema_version != 1 {
            return Err(TimeSeriesError::UnsupportedProjectionSchema);
        }
        if projection.projection_id.trim().is_empty() || projection.sequence == 0 {
            return Err(TimeSeriesError::InvalidProjectionIdentity);
        }
        let observed_at = parse(&projection.observed_at, "projection.observedAt")?;
        let mut seen = std::collections::HashSet::new();
        let mut total_points = 0usize;
        let mut replacements = Vec::with_capacity(projection.series.len());
        for mut projected in projection.series {
            if !seen.insert(projected.id.clone()) {
                return Err(TimeSeriesError::InvalidProjectionSeries);
            }
            let descriptor = self
                .descriptors
                .get(&projected.id)
                .cloned()
                .ok_or(TimeSeriesError::InvalidProjectionSeries)?;
            total_points = total_points
                .checked_add(projected.points.len())
                .ok_or(TimeSeriesError::ProjectionTooLarge)?;
            if total_points > 1_000_000 {
                return Err(TimeSeriesError::ProjectionTooLarge);
            }
            validate_and_sort_points(&mut projected.points, Some(observed_at))?;
            replacements.push((
                projected.id,
                TimeSeriesData {
                    descriptor,
                    points: projected.points,
                },
            ));
        }
        if replacements.is_empty() {
            return Err(TimeSeriesError::InvalidProjectionSeries);
        }

        let mut state = self.state.write().map_err(|_| TimeSeriesError::Poisoned)?;
        if state
            .projection_sequences
            .get(&projection.projection_id)
            .is_some_and(|committed| *committed >= projection.sequence)
        {
            return Err(TimeSeriesError::StaleProjection);
        }
        for (id, data) in replacements {
            state.series.insert(id, data);
        }
        state
            .projection_sequences
            .insert(projection.projection_id, projection.sequence);
        state.sequence = state.sequence.saturating_add(1);
        Ok(state.sequence)
    }
}

impl TimeSeriesPort for InMemoryTimeSeriesPort {
    fn catalog(&self) -> Result<Vec<TimeSeriesDescriptor>, TimeSeriesError> {
        Ok(self.descriptors.values().cloned().collect())
    }

    fn forecast_bundles(&self) -> Result<Vec<ForecastBundle>, TimeSeriesError> {
        Ok(self.bundles.clone())
    }

    fn query(&self, request: &TimeSeriesRequest) -> Result<TimeSeriesWindow, TimeSeriesError> {
        if request.context.account_id != self.account_id {
            return Err(TimeSeriesError::ContextMismatch);
        }
        if !(2..=5_000).contains(&request.max_points) {
            return Err(TimeSeriesError::InvalidPointBudget);
        }
        let from = parse(&request.from, "from")?;
        let to = parse(&request.to, "to")?;
        if to <= from {
            return Err(TimeSeriesError::EmptyWindow);
        }

        let state = self.state.read().map_err(|_| TimeSeriesError::Poisoned)?;
        let mut series = Vec::with_capacity(request.series_ids.len());
        for id in &request.series_ids {
            let Some(descriptor) = self.descriptors.get(id) else {
                continue;
            };
            let data = state.series.get(id);
            let mut points = Vec::new();
            if let Some(data) = data {
                for point in &data.points {
                    let timestamp = parse(point.timestamp(), "point.timestamp")?;
                    if timestamp >= from && timestamp <= to {
                        points.push(point.clone());
                    }
                }
            }
            series.push(TimeSeriesData {
                descriptor: descriptor.clone(),
                points: uniformly_bound(points, request.max_points),
            });
        }
        let mut markers = Vec::new();
        for marker in &state.markers {
            let timestamp = parse(&marker.timestamp, "marker.timestamp")?;
            let available_at = parse(&marker.available_at, "marker.availableAt")?;
            if available_at < timestamp {
                return Err(TimeSeriesError::InvalidTimestamp("marker.availableAt"));
            }
            if timestamp >= from && timestamp <= to {
                markers.push(marker.clone());
            }
        }
        Ok(TimeSeriesWindow {
            schema_version: 2,
            sequence: state.sequence,
            from: request.from.clone(),
            to: request.to.clone(),
            series,
            markers,
        })
    }
}

fn parse(value: &str, field: &'static str) -> Result<OffsetDateTime, TimeSeriesError> {
    OffsetDateTime::parse(value, &Rfc3339).map_err(|_| TimeSeriesError::InvalidTimestamp(field))
}

fn validate_point(
    point: &TimeSeriesPoint,
    projection_observed_at: Option<OffsetDateTime>,
) -> Result<(), TimeSeriesError> {
    let timestamp = parse(point.timestamp(), "point.timestamp")?;
    let (available_at, finite_and_well_formed) = match point {
        TimeSeriesPoint::Scalar {
            available_at,
            value,
            ..
        } => (available_at, value.is_finite()),
        TimeSeriesPoint::Ohlc {
            available_at,
            open,
            high,
            low,
            close,
            ..
        } => (
            available_at,
            [open, high, low, close]
                .iter()
                .all(|value| value.is_finite())
                && *high >= open.max(*close)
                && *low <= open.min(*close),
        ),
    };
    if !finite_and_well_formed {
        return Err(TimeSeriesError::InvalidPointValue);
    }
    let available_at = parse(available_at, "point.availableAt")?;
    if available_at < timestamp
        || projection_observed_at.is_some_and(|observed_at| available_at > observed_at)
    {
        return Err(TimeSeriesError::InvalidTimestamp("point.availableAt"));
    }
    Ok(())
}

fn validate_and_sort_points(
    points: &mut [TimeSeriesPoint],
    projection_observed_at: Option<OffsetDateTime>,
) -> Result<(), TimeSeriesError> {
    for point in points.iter() {
        validate_point(point, projection_observed_at)?;
    }
    points.sort_by(|left, right| left.timestamp().cmp(right.timestamp()));
    if points
        .windows(2)
        .any(|window| window[0].timestamp() == window[1].timestamp())
    {
        return Err(TimeSeriesError::InvalidProjectionSeries);
    }
    Ok(())
}

fn validate_and_sort_markers(markers: &mut [TimelineMarker]) -> Result<(), TimeSeriesError> {
    for marker in markers.iter() {
        let timestamp = parse(&marker.timestamp, "marker.timestamp")?;
        let available_at = parse(&marker.available_at, "marker.availableAt")?;
        if available_at < timestamp {
            return Err(TimeSeriesError::InvalidTimestamp("marker.availableAt"));
        }
    }
    markers.sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
    Ok(())
}

fn uniformly_bound(points: Vec<TimeSeriesPoint>, max_points: usize) -> Vec<TimeSeriesPoint> {
    if points.len() <= max_points {
        return points;
    }
    let last = points.len() - 1;
    (0..max_points)
        .map(|index| {
            let source = index.saturating_mul(last) / (max_points - 1);
            points[source].clone()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DataClass, FeedMode, OperationsContext, SeriesDomain, SeriesRender};

    fn context(account_id: &str) -> OperationsContext {
        OperationsContext {
            organization_id: "org".into(),
            organization_name: "Org".into(),
            workspace_id: "workspace".into(),
            workspace_name: "Workspace".into(),
            account_id: account_id.into(),
            account_name: "Paper".into(),
        }
    }

    fn descriptor() -> TimeSeriesDescriptor {
        TimeSeriesDescriptor {
            id: "mark".into(),
            label: "Mark".into(),
            short_label: "Mark".into(),
            domain: SeriesDomain::Market,
            unit: "USD".into(),
            precision: 2,
            color: "#fff".into(),
            render: SeriesRender::Line,
            provenance: "paper feed".into(),
            source_names: None,
            freshness: "stream".into(),
            default_visible: true,
            pane_weight: None,
        }
    }

    fn point(second: usize) -> TimeSeriesPoint {
        TimeSeriesPoint::Scalar {
            timestamp: format!("2026-08-31T00:00:{second:02}Z"),
            available_at: format!("2026-08-31T00:00:{second:02}Z"),
            value: second as f64,
            color: None,
        }
    }

    #[test]
    fn query_is_context_scoped_and_bounded_without_mutating_truth() {
        let port = InMemoryTimeSeriesPort::new("paper-1", vec![descriptor()], vec![]);
        port.replace_series("mark", (0..10).map(point).collect())
            .unwrap();
        let request = TimeSeriesRequest {
            context: context("paper-1"),
            series_ids: vec!["mark".into()],
            from: "2026-08-31T00:00:00Z".into(),
            to: "2026-08-31T00:00:09Z".into(),
            max_points: 4,
        };
        let first = port.query(&request).unwrap();
        assert_eq!(first.series[0].points.len(), 4);
        assert_eq!(
            first.series[0].points[0].timestamp(),
            "2026-08-31T00:00:00Z"
        );
        assert_eq!(
            first.series[0].points[3].timestamp(),
            "2026-08-31T00:00:09Z"
        );

        let all = port
            .query(&TimeSeriesRequest {
                max_points: 10,
                ..request
            })
            .unwrap();
        assert_eq!(all.series[0].points.len(), 10);
    }

    #[test]
    fn foreign_account_and_invalid_budget_fail_closed() {
        let port = InMemoryTimeSeriesPort::new("paper-1", vec![descriptor()], vec![]);
        let base = TimeSeriesRequest {
            context: context("other"),
            series_ids: vec![],
            from: "2026-08-31T00:00:00Z".into(),
            to: "2026-08-31T00:00:09Z".into(),
            max_points: 4,
        };
        assert_eq!(port.query(&base), Err(TimeSeriesError::ContextMismatch));
        assert_eq!(
            port.query(&TimeSeriesRequest {
                context: context("paper-1"),
                max_points: 1,
                ..base
            }),
            Err(TimeSeriesError::InvalidPointBudget)
        );
    }

    #[test]
    fn marker_filter_never_promotes_out_of_window_events() {
        let port = InMemoryTimeSeriesPort::new("paper-1", vec![descriptor()], vec![]);
        port.replace_markers(vec![TimelineMarker {
            id: "late".into(),
            timestamp: "2026-08-31T01:00:00Z".into(),
            available_at: "2026-08-31T01:00:01Z".into(),
            kind: crate::types::MarkerKind::Alert,
            label: "Late".into(),
            entity_id: "source".into(),
            detail: "outside".into(),
            attributes: BTreeMap::new(),
        }])
        .unwrap();
        let result = port
            .query(&TimeSeriesRequest {
                context: context("paper-1"),
                series_ids: vec![],
                from: "2026-08-31T00:00:00Z".into(),
                to: "2026-08-31T00:00:09Z".into(),
                max_points: 4,
            })
            .unwrap();
        assert!(result.markers.is_empty());
    }

    #[test]
    fn malformed_or_causally_invalid_markers_fail_closed() {
        let port = InMemoryTimeSeriesPort::new("paper-1", vec![descriptor()], vec![]);
        for (timestamp, available_at) in [
            ("not-a-time", "2026-08-31T00:00:01Z"),
            ("2026-08-31T00:00:02Z", "2026-08-31T00:00:01Z"),
        ] {
            assert!(matches!(
                port.replace_markers(vec![TimelineMarker {
                    id: "invalid".into(),
                    timestamp: timestamp.into(),
                    available_at: available_at.into(),
                    kind: crate::types::MarkerKind::Alert,
                    label: "Invalid".into(),
                    entity_id: "source".into(),
                    detail: "must fail".into(),
                    attributes: BTreeMap::new(),
                }]),
                Err(TimeSeriesError::InvalidTimestamp(_))
            ));
        }
    }

    #[test]
    fn append_is_ordered_upserting_and_memory_bounded() {
        let port = InMemoryTimeSeriesPort::new("paper-1", vec![descriptor()], vec![]);
        port.append_point("mark", point(2), 2).unwrap();
        port.append_point("mark", point(1), 2).unwrap();
        port.append_point("mark", point(2), 2).unwrap();
        port.append_point("mark", point(3), 2).unwrap();
        let window = port
            .query(&TimeSeriesRequest {
                context: context("paper-1"),
                series_ids: vec!["mark".into()],
                from: "2026-08-31T00:00:00Z".into(),
                to: "2026-08-31T00:00:09Z".into(),
                max_points: 10,
            })
            .unwrap();
        assert_eq!(window.series[0].points, vec![point(2), point(3)]);
    }

    #[test]
    fn imports_are_not_accidental_domain_dependencies() {
        let _ = (FeedMode::Paper, DataClass::Observed);
    }

    #[test]
    fn projection_replacement_is_atomic_causal_and_monotonic() {
        let port = InMemoryTimeSeriesPort::new("paper-1", vec![descriptor()], vec![]);
        let projection = TimeSeriesProjection {
            schema_version: 1,
            projection_id: "shadow".into(),
            sequence: 9,
            observed_at: "2026-08-31T00:00:05Z".into(),
            series: vec![crate::types::ProjectedTimeSeries {
                id: "mark".into(),
                points: vec![point(2), point(1)],
            }],
        };
        port.replace_projection(projection.clone()).unwrap();
        assert_eq!(
            port.replace_projection(projection),
            Err(TimeSeriesError::StaleProjection)
        );
        let window = port
            .query(&TimeSeriesRequest {
                context: context("paper-1"),
                series_ids: vec!["mark".into()],
                from: "2026-08-31T00:00:00Z".into(),
                to: "2026-08-31T00:00:09Z".into(),
                max_points: 10,
            })
            .unwrap();
        assert_eq!(window.series[0].points, vec![point(1), point(2)]);

        let invalid = TimeSeriesProjection {
            schema_version: 1,
            projection_id: "other".into(),
            sequence: 1,
            observed_at: "2026-08-31T00:00:01Z".into(),
            series: vec![crate::types::ProjectedTimeSeries {
                id: "mark".into(),
                points: vec![point(2)],
            }],
        };
        assert_eq!(
            port.replace_projection(invalid),
            Err(TimeSeriesError::InvalidTimestamp("point.availableAt"))
        );
    }
}

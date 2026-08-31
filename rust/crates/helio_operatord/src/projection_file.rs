use crate::time_series::{InMemoryTimeSeriesPort, TimeSeriesError};
use crate::types::TimeSeriesProjection;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum ProjectionFileError {
    #[error("cannot read time-series projection: {0}")]
    Io(#[from] std::io::Error),
    #[error("time-series projection is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("time-series projection was rejected: {0}")]
    Projection(#[from] TimeSeriesError),
}

pub fn load_projection_file(
    path: &Path,
    port: &InMemoryTimeSeriesPort,
) -> Result<[u8; 32], ProjectionFileError> {
    let raw = std::fs::read(path)?;
    apply_projection_bytes(&raw, port)?;
    Ok(Sha256::digest(&raw).into())
}

pub async fn watch_projection_file(
    path: PathBuf,
    port: Arc<InMemoryTimeSeriesPort>,
    interval: Duration,
    mut committed_digest: [u8; 32],
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ticker.tick().await;
    loop {
        ticker.tick().await;
        match tokio::fs::read(&path).await {
            Ok(raw) => {
                let digest: [u8; 32] = Sha256::digest(&raw).into();
                if digest == committed_digest {
                    continue;
                }
                match apply_projection_bytes(&raw, port.as_ref()) {
                    Ok(()) => {
                        committed_digest = digest;
                        info!(path = %path.display(), "time-series projection advanced");
                    }
                    Err(error) => {
                        warn!(path = %path.display(), error = %error, "time-series projection update rejected");
                    }
                }
            }
            Err(error) => {
                warn!(path = %path.display(), error = %error, "time-series projection file unavailable");
            }
        }
    }
}

fn apply_projection_bytes(
    raw: &[u8],
    port: &InMemoryTimeSeriesPort,
) -> Result<(), ProjectionFileError> {
    let projection: TimeSeriesProjection = serde_json::from_slice(raw)?;
    port.replace_projection(projection)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        SeriesDomain, SeriesRender, TimeSeriesDescriptor, TimeSeriesPoint, TimeSeriesRequest,
    };
    use crate::TimeSeriesPort;

    #[test]
    fn file_loader_commits_valid_projection() {
        let port = InMemoryTimeSeriesPort::new(
            "paper",
            vec![TimeSeriesDescriptor {
                id: "source".into(),
                label: "Source".into(),
                short_label: "Source".into(),
                domain: SeriesDomain::Source,
                unit: "nT".into(),
                precision: 2,
                color: "#fff".into(),
                render: SeriesRender::Line,
                provenance: "test".into(),
                source_names: None,
                freshness: "test".into(),
                default_visible: false,
                pane_weight: None,
            }],
            vec![],
        );
        let directory =
            std::env::temp_dir().join(format!("helios-projection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("projection.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schemaVersion": 1,
                "projectionId": "shadow",
                "sequence": 1,
                "observedAt": "2026-08-31T00:00:02Z",
                "series": [{
                    "id": "source",
                    "points": [{
                        "kind": "scalar",
                        "timestamp": "2026-08-31T00:00:00Z",
                        "availableAt": "2026-08-31T00:00:01Z",
                        "value": -3.2
                    }]
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        let digest = load_projection_file(&path, &port).unwrap();
        assert_ne!(digest, [0; 32]);
        let result = port
            .query(&TimeSeriesRequest {
                context: crate::types::OperationsContext {
                    organization_id: "org".into(),
                    organization_name: "Org".into(),
                    workspace_id: "workspace".into(),
                    workspace_name: "Workspace".into(),
                    account_id: "paper".into(),
                    account_name: "Paper".into(),
                },
                series_ids: vec!["source".into()],
                from: "2026-08-31T00:00:00Z".into(),
                to: "2026-08-31T00:00:02Z".into(),
                max_points: 10,
            })
            .unwrap();
        assert!(matches!(
            result.series[0].points.as_slice(),
            [TimeSeriesPoint::Scalar { value, .. }] if *value == -3.2
        ));
    }
}

use crate::store::now;
use crate::types::*;

pub fn empty_snapshot() -> OperationsSnapshot {
    OperationsSnapshot {
        schema_version: 2,
        sequence: 1,
        mode: FeedMode::Paper,
        provider: "helio-operatord".into(),
        observed_at: now().unwrap_or_else(|_| "1970-01-01T00:00:00Z".into()),
        data_class: DataClass::Observed,
        context: OperationsContext {
            organization_id: "local".into(),
            organization_name: "Local operator".into(),
            workspace_id: "paper".into(),
            workspace_name: "Paper operations".into(),
            account_id: "paper-primary".into(),
            account_name: "Paper primary".into(),
        },
        strategies: vec![],
        stages: vec![],
        signals: vec![],
        positions: vec![],
        orders: vec![],
        fills: vec![],
        sources: vec![],
        alerts: vec![],
        metrics: vec![],
        activity: vec![],
        risk: RiskView {
            gross_exposure_micros: "0".into(),
            gross_limit_micros: "100000000".into(),
            reserved_gross_micros: "0".into(),
            daily_order_count: 0,
            daily_order_limit: 10,
            pending_reconciliations: 0,
            open_incidents: 0,
            kill_switch_active: false,
            capital_gate: CapitalGate::Closed,
            capital_gate_reason: "Paper broker and source reconciliation have not been certified"
                .into(),
            checkpoint_age_ms: 0,
            source_lag_ms: 0,
            clock_offset_ms: 0,
        },
    }
}

pub fn default_catalog() -> Vec<TimeSeriesDescriptor> {
    vec![
        descriptor(
            "market-ohlc",
            "Price",
            "Price",
            SeriesDomain::Market,
            "USD",
            4,
            "#78a9ef",
            SeriesRender::Candlestick,
            "Consolidated paper mark",
            Some(vec!["market".into()]),
            true,
        ),
        descriptor(
            "market-volume",
            "Volume",
            "Volume",
            SeriesDomain::Market,
            "shares",
            0,
            "#4f94ee",
            SeriesRender::Histogram,
            "Paper trade tape",
            Some(vec!["market".into()]),
            true,
        ),
        descriptor(
            "goes-xray-flux",
            "GOES X-ray flux",
            "X-ray",
            SeriesDomain::Source,
            "W/m2",
            9,
            "#f5bf42",
            SeriesRender::Line,
            "NOAA SWPC GOES primary X-ray flux",
            Some(vec!["noaa-swpc-goes-xray-primary-v1".into()]),
            false,
        ),
        descriptor(
            "goes-proton-flux-ge10",
            "GOES proton flux",
            "Protons",
            SeriesDomain::Source,
            "pfu",
            3,
            "#ed7655",
            SeriesRender::Line,
            "NOAA SWPC GOES primary integral proton flux at or above 10 MeV",
            Some(vec!["noaa-swpc-goes-protons-primary-v1".into()]),
            false,
        ),
        descriptor(
            "donki-flare-events",
            "DONKI solar flares",
            "Flares",
            SeriesDomain::Source,
            "event",
            0,
            "#ff9e64",
            SeriesRender::Histogram,
            "NASA CCMC DONKI flare revisions",
            Some(vec!["nasa-ccmc-donki-flare-v1".into()]),
            false,
        ),
        descriptor(
            "donki-cme-analysis",
            "DONKI CME analysis",
            "CME",
            SeriesDomain::Source,
            "km/s",
            0,
            "#db6d9b",
            SeriesRender::Histogram,
            "NASA CCMC DONKI CME and WSA-ENLIL revisions",
            Some(vec!["nasa-ccmc-donki-cme-v1".into()]),
            false,
        ),
        descriptor(
            "l1-solar-wind-speed",
            "Solar wind speed",
            "Wind",
            SeriesDomain::Source,
            "km/s",
            2,
            "#46c7d7",
            SeriesRender::Line,
            "NOAA SWPC active L1 real-time solar-wind source",
            Some(vec!["noaa-swpc-l1-wind-1m-v1".into()]),
            false,
        ),
        descriptor(
            "l1-imf-bz-gsm",
            "Interplanetary magnetic field Bz",
            "IMF Bz",
            SeriesDomain::Source,
            "nT",
            2,
            "#cf73ff",
            SeriesRender::Baseline,
            "NOAA SWPC active L1 real-time magnetometer source",
            Some(vec!["noaa-swpc-l1-mag-1m-v1".into()]),
            false,
        ),
        descriptor(
            "planetary-kp",
            "Planetary Kp",
            "Kp",
            SeriesDomain::Source,
            "index",
            2,
            "#d6c05c",
            SeriesRender::Line,
            "NOAA SWPC one-minute planetary K index",
            Some(vec!["noaa-swpc-planetary-kp-1m-v1".into()]),
            false,
        ),
        descriptor(
            "source-latency",
            "Source latency p95",
            "Latency",
            SeriesDomain::Source,
            "ms",
            0,
            "#e17455",
            SeriesRender::Line,
            "Operator source telemetry",
            Some(vec!["operator-telemetry".into()]),
            false,
        ),
        descriptor(
            "source-quality",
            "Required source quality",
            "Quality",
            SeriesDomain::Source,
            "%",
            2,
            "#63c9d4",
            SeriesRender::Line,
            "Operator freshness and quality gates",
            Some(vec!["operator-telemetry".into()]),
            false,
        ),
        descriptor(
            "risk-utilization",
            "Gross risk utilization",
            "Risk",
            SeriesDomain::Risk,
            "%",
            2,
            "#e5a12c",
            SeriesRender::Line,
            "Independent risk authority",
            None,
            true,
        ),
        descriptor(
            "net-exposure",
            "Net exposure",
            "Exposure",
            SeriesDomain::Portfolio,
            "USD",
            2,
            "#47c2cf",
            SeriesRender::Area,
            "Fill-derived position projection",
            None,
            true,
        ),
    ]
}

pub fn default_forecast_bundles() -> Vec<ForecastBundle> {
    const SPACE_WEATHER: &[u8] =
        include_bytes!("../../../../config/forecasts/space-weather-impact-v1.json");
    const EXECUTION: &[u8] =
        include_bytes!("../../../../config/forecasts/execution-evidence-v1.json");
    [SPACE_WEATHER, EXECUTION]
        .into_iter()
        .map(|manifest| {
            ForecastBundle::from_manifest_bytes(manifest)
                .expect("embedded forecast bundle must satisfy the versioned contract")
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn descriptor(
    id: &str,
    label: &str,
    short_label: &str,
    domain: SeriesDomain,
    unit: &str,
    precision: u8,
    color: &str,
    render: SeriesRender,
    provenance: &str,
    source_names: Option<Vec<String>>,
    default_visible: bool,
) -> TimeSeriesDescriptor {
    TimeSeriesDescriptor {
        id: id.into(),
        label: label.into(),
        short_label: short_label.into(),
        domain,
        unit: unit.into(),
        precision,
        color: color.into(),
        render,
        provenance: provenance.into(),
        source_names,
        freshness: "not connected".into(),
        default_visible,
        pane_weight: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn embedded_forecasts_are_fingerprinted_and_resolve_to_the_catalog() {
        let catalog: HashSet<_> = default_catalog()
            .into_iter()
            .map(|descriptor| descriptor.id)
            .collect();
        for bundle in default_forecast_bundles() {
            assert_eq!(bundle.schema_version, 1);
            assert_eq!(bundle.definition_sha256.len(), 64);
            assert!(bundle.series_ids.iter().all(|id| catalog.contains(id)));
            assert_eq!(
                bundle.series_ids,
                bundle
                    .input_contract
                    .iter()
                    .map(|input| input.series_id.clone())
                    .collect::<Vec<_>>()
            );
        }
    }
}

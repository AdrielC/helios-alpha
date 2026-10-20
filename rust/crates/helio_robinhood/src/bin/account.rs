//! One read-only account request, with credentials received only through bounded STDIN.

use std::io::{self, Read};
use std::time::Duration;

use helio_execution::BrokerDecimal;
use helio_robinhood::{
    CryptoJournalBinding, Ed25519RequestSigner, HttpMethod, HttpRequest, HttpResponse,
    ReqwestRobinhoodTransport, RobinhoodCryptoBroker, RobinhoodCryptoConfig, RobinhoodTransport,
    SystemUnixClock, TransportError, UnixClock,
};
use serde::Deserialize;
use serde_json::json;
use zeroize::Zeroize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema_version: u32,
    account_number: String,
    journal_account_key: String,
    api_key: String,
    private_key_base64: String,
    #[serde(default)]
    market: Option<MarketRequest>,
    #[serde(default)]
    markets: Option<Vec<MarketRequest>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MarketRequest {
    symbol: String,
    quantity: BrokerDecimal,
}

struct AccountTransport {
    inner: ReqwestRobinhoodTransport,
    gets: u32,
    max_gets: u32,
}

impl RobinhoodTransport for AccountTransport {
    fn execute(&mut self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let path = request.path_and_query.split('?').next().unwrap_or_default();
        if request.method != HttpMethod::Get
            || !request.body.is_empty()
            || self.gets >= self.max_gets
            || !matches!(
                path,
                "/api/v2/crypto/trading/accounts/"
                    | "/api/v2/crypto/trading/holdings/"
                    | "/api/v2/crypto/trading/trading_pairs/"
                    | "/api/v2/crypto/trading/estimated_price/"
            )
        {
            return Err(TransportError::Unavailable);
        }
        self.gets += 1;
        self.inner.execute(request)
    }
}

fn market_plan(
    single: Option<MarketRequest>,
    batch: Option<Vec<MarketRequest>>,
) -> Result<(bool, Vec<MarketRequest>), &'static str> {
    if single.is_some() && batch.is_some() {
        return Err("market_request");
    }
    let is_batch = batch.is_some();
    let requests = batch.unwrap_or_else(|| single.into_iter().collect());
    if requests.len() > 2
        || requests.iter().any(|r| {
            !matches!(r.symbol.as_str(), "BTC-USD" | "ETH-USD")
                || !r
                    .quantity
                    .as_str()
                    .bytes()
                    .any(|b| (b'1'..=b'9').contains(&b))
        })
        || requests
            .iter()
            .map(|r| &r.symbol)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != requests.len()
    {
        return Err("market_request");
    }
    Ok((is_batch, requests))
}

fn run() -> Result<serde_json::Value, &'static str> {
    let mut raw = Vec::new();
    io::stdin()
        .take(65_537)
        .read_to_end(&mut raw)
        .map_err(|_| "input")?;
    if raw.len() > 65_536 {
        raw.zeroize();
        return Err("input_bound");
    }
    let parsed = serde_json::from_slice::<Request>(&raw);
    raw.zeroize();
    let mut request = parsed.map_err(|_| "request_shape")?;
    if request.schema_version != 1 {
        request.private_key_base64.zeroize();
        return Err("request_version");
    }
    let planned = market_plan(request.market.take(), request.markets.take());
    let (is_batch, markets) = match planned {
        Ok(value) => value,
        Err(error) => {
            request.private_key_base64.zeroize();
            return Err(error);
        }
    };
    let signer = Ed25519RequestSigner::from_base64(request.api_key, &request.private_key_base64);
    request.private_key_base64.zeroize();
    let signer = signer.map_err(|_| "credentials")?;
    let binding =
        CryptoJournalBinding::try_new(&request.account_number, &request.journal_account_key)
            .map_err(|_| "binding")?;
    let config = RobinhoodCryptoConfig::try_new(request.account_number).map_err(|_| "account")?;
    let transport = AccountTransport {
        inner: ReqwestRobinhoodTransport::try_new(Duration::from_secs(3))
            .map_err(|_| "transport")?,
        gets: 0,
        max_gets: 8 + 5 * markets.len() as u32,
    };
    let mut broker = RobinhoodCryptoBroker::try_new(config, transport, SystemUnixClock, signer)
        .map_err(|_| "configuration")?;
    let availability = match broker.read_crypto_account(&binding) {
        Ok(value) => value,
        Err(_) => {
            return Ok(
                json!({"error": "account_unavailable", "get_requests": broker.transport().gets}),
            )
        }
    };
    let mut oldest_ms = availability.data().read_started_ms;
    let mut result = json!({"availability": availability});
    let mut estimates = Vec::with_capacity(markets.len());
    for market in markets {
        match broker.read_crypto_market(&binding, &market.symbol, &market.quantity) {
            Ok(value) => {
                oldest_ms = oldest_ms.min(value.data().quote.observed_ms);
                estimates.push(serde_json::to_value(value).map_err(|_| "market_shape")?)
            }
            Err(_) => {
                return Ok(
                    json!({"error": "market_unavailable", "get_requests": broker.transport().gets}),
                )
            }
        }
    }
    let completed_ms = SystemUnixClock.unix_millis().map_err(|_| "clock")?;
    if completed_ms
        .checked_sub(oldest_ms)
        .is_none_or(|age| age >= 5_000)
    {
        return Ok(json!({"error": "read_set_expired", "get_requests": broker.transport().gets}));
    }
    if is_batch {
        result["markets"] = estimates.into();
    } else if let Some(estimate) = estimates.pop() {
        result["market"] = estimate;
    }
    result["get_requests"] = broker.transport().gets.into();
    Ok(result)
}

fn main() {
    let mut result = run().unwrap_or_else(|error| json!({"error": error}));
    result["schema_version"] = 1.into();
    println!("{result}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_reader_rejects_all_writes_and_order_endpoints_before_transport() {
        let mut transport = AccountTransport {
            inner: ReqwestRobinhoodTransport::try_new(Duration::from_secs(1)).unwrap(),
            gets: 0,
            max_gets: 8,
        };
        for (method, path) in [
            (HttpMethod::Post, "/api/v2/crypto/trading/accounts/"),
            (HttpMethod::Post, "/api/v2/crypto/trading/holdings/"),
            (HttpMethod::Post, "/api/v2/crypto/trading/orders/"),
            (HttpMethod::Get, "/api/v2/crypto/trading/orders/"),
            (HttpMethod::Post, "/api/v2/crypto/trading/trading_pairs/"),
            (HttpMethod::Post, "/api/v2/crypto/trading/estimated_price/"),
            (HttpMethod::Get, "/api/v1/crypto/marketdata/best_bid_ask/"),
        ] {
            assert!(transport
                .execute(HttpRequest {
                    method,
                    path_and_query: path.into(),
                    headers: Default::default(),
                    body: Vec::new(),
                })
                .is_err());
        }
        assert_eq!(transport.gets, 0);
    }

    #[test]
    fn market_batches_are_bounded_unique_and_validated_before_any_broker_read() {
        let market = |symbol: &str, quantity: &str| MarketRequest {
            symbol: symbol.into(),
            quantity: BrokerDecimal::try_new(quantity).unwrap(),
        };
        assert!(market_plan(Some(market("BTC-USD", "1")), Some(vec![])).is_err());
        assert!(market_plan(
            None,
            Some(vec![market("BTC-USD", "1"), market("BTC-USD", "2")])
        )
        .is_err());
        assert!(market_plan(None, Some(vec![market("BTC-USD", "0.00")])).is_err());
        assert!(market_plan(None, Some(vec![market("OTHER-USD", "1")])).is_err());
        assert!(market_plan(
            None,
            Some(vec![
                market("BTC-USD", "1"),
                market("ETH-USD", "2"),
                market("BTC-USD", "3")
            ])
        )
        .is_err());
        let (batch, markets) = market_plan(
            None,
            Some(vec![
                market("BTC-USD", "0.000000000000000007"),
                market("ETH-USD", "0.5"),
            ]),
        )
        .unwrap();
        assert!(batch);
        assert_eq!(markets.len(), 2);
        assert!(market_plan(None, Some(vec![])).unwrap().0);
        assert!(!market_plan(None, None).unwrap().0);
    }
}

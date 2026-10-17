use crate::error::{ErrorMessage, HttpError};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use moka::future::Cache;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Fixed-window request counter per client IP. A window starts with the
/// client's first request and lasts `window`.
#[derive(Clone)]
pub struct RateLimiter {
    hits: Cache<IpAddr, Arc<AtomicU32>>,
    max_requests: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            hits: Cache::builder()
                .max_capacity(100_000)
                .time_to_live(window)
                .build(),
            max_requests,
            window,
        }
    }

    pub async fn allow(&self, ip: IpAddr) -> bool {
        let counter = self
            .hits
            .get_with(ip, async { Arc::new(AtomicU32::new(0)) })
            .await;
        counter.fetch_add(1, Ordering::Relaxed) < self.max_requests
    }
}

pub async fn limit_by_ip(
    State(limiter): State<RateLimiter>,
    request: Request,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(addr)| addr.ip());
    let ip = client_ip(peer, request.headers());

    if limiter.allow(ip).await {
        return next.run(request).await;
    }

    tracing::warn!(%ip, path = %request.uri().path(), "rate limit exceeded");
    let mut response = HttpError::from(ErrorMessage::TooManyRequests).into_http_response();
    if let Ok(retry_after) = HeaderValue::from_str(&limiter.window.as_secs().to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, retry_after);
    }
    response
}

/// The peer address, or `X-Real-IP` when the peer is a local reverse proxy.
/// Clients reaching the server directly cannot spoof their address this way.
fn client_ip(peer: Option<IpAddr>, headers: &HeaderMap) -> IpAddr {
    let Some(peer) = peer else {
        return IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    };
    if !is_local(peer) {
        return peer;
    }
    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(peer)
}

fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback() || v4.is_private())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_real_ip(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_str(value).unwrap());
        headers
    }

    #[tokio::test]
    async fn blocks_after_limit_per_ip() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();
        assert!(limiter.allow(a).await);
        assert!(limiter.allow(a).await);
        assert!(!limiter.allow(a).await);
        assert!(limiter.allow(b).await);
    }

    #[test]
    fn real_ip_header_is_trusted_only_from_local_proxy() {
        let headers = headers_with_real_ip("198.51.100.7");
        let proxy: IpAddr = "127.0.0.1".parse().unwrap();
        let docker_gateway: IpAddr = "172.17.0.1".parse().unwrap();
        let internet: IpAddr = "203.0.113.9".parse().unwrap();

        assert_eq!(client_ip(Some(proxy), &headers), "198.51.100.7".parse::<IpAddr>().unwrap());
        assert_eq!(client_ip(Some(docker_gateway), &headers), "198.51.100.7".parse::<IpAddr>().unwrap());
        assert_eq!(client_ip(Some(internet), &headers), internet);
    }

    #[test]
    fn malformed_real_ip_falls_back_to_peer() {
        let proxy: IpAddr = "127.0.0.1".parse().unwrap();
        assert_eq!(client_ip(Some(proxy), &headers_with_real_ip("not-an-ip")), proxy);
    }
}

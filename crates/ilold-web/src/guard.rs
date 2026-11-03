use std::net::{IpAddr, SocketAddr};

use axum::body::Body;
use axum::extract::connect_info::{ConnectInfo, Connected};
use axum::http::uri::Authority;
use axum::http::{header, Request, Response, StatusCode, Uri};
use axum::serve::IncomingStream;
use tokio::net::TcpListener;

/// Names this machine when the server listens on every interface.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalAddr(Option<IpAddr>);

impl Connected<IncomingStream<'_, TcpListener>> for LocalAddr {
    fn connect_info(stream: IncomingStream<'_, TcpListener>) -> Self {
        LocalAddr(stream.io().local_addr().ok().map(|addr| addr.ip()))
    }
}

/// Stops other web pages from driving the API or the terminal: DNS rebinding through Host, cross-site requests through Origin.
#[derive(Clone, Debug)]
pub(crate) struct RequestGuard {
    bound: SocketAddr,
    dev_origin: Option<String>,
}

impl RequestGuard {
    pub(crate) fn new(bound: SocketAddr, dev_origin: Option<String>) -> Self {
        let dev_origin = dev_origin.filter(|origin| is_web_origin(origin.trim_end_matches('/')));
        Self { bound, dev_origin }
    }

    pub(crate) fn check(&self, request: &Request<Body>) -> Result<(), Response<Body>> {
        let headers = request.headers();
        let repeated = [header::HOST.as_str(), header::ORIGIN.as_str(), "sec-fetch-site"]
            .iter()
            .any(|name| headers.get_all(*name).iter().count() > 1);
        let local = request.extensions().get::<ConnectInfo<LocalAddr>>().and_then(|info| info.0 .0);
        let host = headers.get(header::HOST).and_then(|value| value.to_str().ok());
        // browsers label subresource requests; WebSocket upgrades rely on the Origin check
        let cross_site = headers.get("sec-fetch-site")
            .is_some_and(|site| matches!(site.as_bytes(), b"cross-site" | b"same-site"));
        let allowed = !repeated && !cross_site && host.is_some_and(|host| {
            self.host_allowed(host, local)
                && match request.headers().get(header::ORIGIN) {
                    None => true,
                    Some(origin) => origin.to_str().is_ok_and(|origin| self.origin_allowed(origin, host)),
                }
        });
        if allowed {
            return Ok(());
        }
        let mut response = Response::new(Body::from("Forbidden: unexpected Host, Origin or cross-site request"));
        *response.status_mut() = StatusCode::FORBIDDEN;
        Err(response)
    }

    fn host_allowed(&self, host: &str, local: Option<IpAddr>) -> bool {
        let Ok(authority) = host.parse::<Authority>() else { return false };
        if authority.port_u16().unwrap_or(80) != self.bound.port() {
            return false;
        }
        let name = authority.host();
        if name.eq_ignore_ascii_case("localhost") {
            return true;
        }
        let Ok(ip) = name.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() else { return false };
        let (ip, bound) = (ip.to_canonical(), self.bound.ip().to_canonical());
        if bound.is_unspecified() {
            ip.is_loopback() || local.map(|local| local.to_canonical()) == Some(ip)
        } else {
            ip.is_loopback() || ip == bound
        }
    }

    fn origin_allowed(&self, origin: &str, host: &str) -> bool {
        if self.dev_origin.as_deref().is_some_and(|dev| dev.trim_end_matches('/').eq_ignore_ascii_case(origin)) {
            return true;
        }
        is_web_origin(origin)
            && origin.parse::<Uri>().is_ok_and(|uri| {
                uri.authority().is_some_and(|authority| authority.as_str().eq_ignore_ascii_case(host))
            })
    }
}

fn is_web_origin(origin: &str) -> bool {
    origin.parse::<Uri>().is_ok_and(|uri| {
        matches!(uri.scheme_str(), Some("http" | "https")) && uri.authority().is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound_to(bound: &str, dev_origin: Option<&str>) -> RequestGuard {
        RequestGuard::new(bound.parse().unwrap(), dev_origin.map(str::to_string))
    }

    #[test]
    fn explicit_bind_address_is_a_valid_host() {
        let guard = bound_to("192.168.1.5:8080", None);
        assert!(guard.host_allowed("192.168.1.5:8080", None));
        assert!(guard.host_allowed("localhost:8080", None));
        assert!(!guard.host_allowed("192.168.1.6:8080", None));
        assert!(!guard.host_allowed("192.168.1.5:8081", None));
        assert!(!guard.host_allowed("evil.example:8080", None));
        assert!(!guard.host_allowed("", None));
    }

    #[test]
    fn unspecified_bind_accepts_the_interface_a_connection_arrived_on() {
        let guard = bound_to("0.0.0.0:8080", None);
        let local = Some("192.168.1.5".parse().unwrap());
        assert!(guard.host_allowed("192.168.1.5:8080", local));
        assert!(guard.host_allowed("127.0.0.1:8080", local));
        assert!(!guard.host_allowed("192.168.1.6:8080", local));
        assert!(!guard.host_allowed("192.168.1.5:8080", None));
        assert!(!guard.host_allowed("0.0.0.0:8080", local));
        let dual = bound_to("[::]:8080", None);
        assert!(dual.host_allowed("192.168.1.5:8080", Some("::ffff:192.168.1.5".parse().unwrap())));
        assert!(dual.host_allowed("[fe80::1]:8080", Some("fe80::1".parse().unwrap())));
    }

    #[test]
    fn default_port_hosts_need_a_port_80_bind() {
        assert!(bound_to("127.0.0.1:80", None).host_allowed("localhost", None));
        assert!(!bound_to("127.0.0.1:8080", None).host_allowed("localhost", None));
    }

    #[test]
    fn origin_must_match_the_host_or_the_configured_dev_origin() {
        let plain = bound_to("127.0.0.1:8080", None);
        assert!(plain.origin_allowed("http://127.0.0.1:8080", "127.0.0.1:8080"));
        assert!(plain.origin_allowed("http://LOCALHOST:8080", "localhost:8080"));
        assert!(!plain.origin_allowed("http://localhost:8080", "127.0.0.1:8080"));
        assert!(!plain.origin_allowed("http://localhost:5173", "localhost:8080"));
        assert!(!plain.origin_allowed("null", "127.0.0.1:8080"));
        assert!(!plain.origin_allowed("file://127.0.0.1:8080", "127.0.0.1:8080"));
        let dev = bound_to("127.0.0.1:8080", Some("http://localhost:5173/"));
        assert!(dev.origin_allowed("http://localhost:5173", "localhost:8080"));
        assert!(!dev.origin_allowed("http://evil.example", "localhost:8080"));
    }

    #[test]
    fn invalid_dev_origins_are_ignored() {
        for value in ["null", "", "*", "garbage", "file:///tmp"] {
            let guard = bound_to("127.0.0.1:8080", Some(value));
            assert!(guard.dev_origin.is_none(), "{value}");
            assert!(!guard.origin_allowed(value, "127.0.0.1:8080"), "{value}");
        }
    }

    #[test]
    fn repeated_host_origin_or_fetch_site_headers_are_refused() {
        let guard = bound_to("127.0.0.1:8080", None);
        let request = |headers: &[(&str, &str)]| {
            let mut builder = Request::builder().uri("/api/project");
            for (name, value) in headers {
                builder = builder.header(*name, *value);
            }
            builder.body(Body::empty()).unwrap()
        };
        assert!(guard.check(&request(&[("host", "127.0.0.1:8080")])).is_ok());
        assert!(guard.check(&request(&[("host", "127.0.0.1:8080"), ("host", "evil:8080")])).is_err());
        let origin = "http://127.0.0.1:8080";
        assert!(guard.check(&request(&[("host", "127.0.0.1:8080"), ("origin", origin), ("origin", "http://evil")])).is_err());
        assert!(guard.check(&request(&[("host", "127.0.0.1:8080"), ("sec-fetch-site", "same-origin"), ("sec-fetch-site", "cross-site")])).is_err());
    }
}

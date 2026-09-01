//! SSRF controls for provider `base_url` values and outbound upstream requests.
//!
//! Validates URLs on catalog writes and re-checks resolved addresses on every
//! outbound request to mitigate DNS rebinding.

use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// Policy for which upstream hosts are permitted.
#[derive(Debug, Clone, Default)]
pub struct UpstreamUrlPolicy {
    /// Hostnames or IP literals that may target private/link-local space.
    pub allow_private_hosts: HashSet<String>,
}

impl UpstreamUrlPolicy {
    pub fn from_allowlist(hosts: &[String]) -> Self {
        Self {
            allow_private_hosts: hosts
                .iter()
                .map(|h| normalize_host(h))
                .filter(|h| !h.is_empty())
                .collect(),
        }
    }

    pub fn is_private_host_allowed(&self, host: &str) -> bool {
        self.allow_private_hosts.contains(&normalize_host(host))
    }

    /// Test helper: permit loopback for wiremock and local integration tests.
    #[cfg(test)]
    pub fn allow_loopback_for_tests() -> Self {
        Self::from_allowlist(&["127.0.0.1".into(), "localhost".into(), "[::1]".into()])
    }
}

/// Validate a provider `base_url` before persisting it.
pub fn validate_provider_base_url(url: &str, policy: &UpstreamUrlPolicy) -> AppResult<()> {
    let parsed = parse_http_url(url)?;
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::BadRequest("provider base_url must include a host".into()))?;

    if host_has_credentials(&parsed) {
        return Err(AppError::BadRequest(
            "provider base_url must not include username or password".into(),
        ));
    }

    if policy.is_private_host_allowed(host) {
        return Ok(());
    }

    if is_blocked_hostname(host) {
        return Err(AppError::BadRequest(format!(
            "provider base_url host '{host}' is not allowed"
        )));
    }

    if let Some(ip) = parse_ip_literal(host) {
        if is_non_public_ip(ip) {
            return Err(AppError::BadRequest(format!(
                "provider base_url must not target private or link-local address {ip}"
            )));
        }
        return Ok(());
    }

    Ok(())
}

/// Re-validate an outbound URL immediately before connecting (DNS rebinding defense).
pub async fn validate_outbound_url(url: &str, policy: &UpstreamUrlPolicy) -> AppResult<()> {
    validate_provider_base_url(url, policy)?;

    let parsed = parse_http_url(url)?;
    let host = parsed.host_str().expect("validated above");

    if policy.is_private_host_allowed(host) {
        return Ok(());
    }

    if let Some(ip) = parse_ip_literal(host) {
        if is_non_public_ip(ip) {
            return Err(AppError::BadRequest(format!(
                "upstream host resolves to non-public address {ip}"
            )));
        }
        return Ok(());
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| {
            AppError::BadRequest(format!("upstream host '{host}' could not be resolved: {err}"))
        })?;

    let mut any = false;
    for addr in addrs {
        any = true;
        if is_non_public_ip(addr.ip()) {
            return Err(AppError::BadRequest(format!(
                "upstream host '{host}' resolves to non-public address {}",
                addr.ip()
            )));
        }
    }

    if !any {
        return Err(AppError::BadRequest(format!(
            "upstream host '{host}' did not resolve to any address"
        )));
    }

    Ok(())
}

fn parse_http_url(url: &str) -> AppResult<Url> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("provider base_url is required".into()));
    }

    let parsed = Url::parse(trimmed).map_err(|err| {
        AppError::BadRequest(format!("provider base_url is not a valid URL: {err}"))
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(AppError::BadRequest(format!(
                "provider base_url must use http or https, not {other}"
            )));
        }
    }

    if parsed.host().is_none() {
        return Err(AppError::BadRequest(
            "provider base_url must include a host".into(),
        ));
    }

    Ok(parsed)
}

fn host_has_credentials(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(stripped) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        stripped.to_string()
    } else {
        host
    }
}

fn parse_ip_literal(host: &str) -> Option<IpAddr> {
    let host = normalize_host(host);
    if host.parse::<Ipv4Addr>().is_ok() || host.parse::<Ipv6Addr>().is_ok() {
        host.parse().ok()
    } else {
        None
    }
}

fn is_blocked_hostname(host: &str) -> bool {
    let host = normalize_host(host);
    if host.is_empty() {
        return true;
    }

    if matches!(
        host.as_str(),
        "localhost"
            | "metadata"
            | "metadata.google.internal"
            | "kubernetes"
            | "kubernetes.default"
            | "kubernetes.default.svc"
            | "kubernetes.default.svc.cluster.local"
    ) {
        return true;
    }

    if host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".svc")
        || host.ends_with(".svc.cluster.local")
        || host.ends_with(".cluster.local")
    {
        return true;
    }

    // Docker Compose / k8s short names (postgres, redis, gateway, …).
    !host.contains('.')
}

fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_non_public_ipv4(v4),
        IpAddr::V6(v6) => is_non_public_ipv6(v6),
    }
}

fn is_non_public_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.octets()[0] == 0
        || metadata_ipv4(ip)
        || cgnat_ipv4(ip)
}

fn is_non_public_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || documentation_ipv6(ip)
}

fn documentation_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn metadata_ipv4(ip: Ipv4Addr) -> bool {
    ip == Ipv4Addr::new(169, 254, 169, 254)
}

fn cgnat_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> UpstreamUrlPolicy {
        UpstreamUrlPolicy::allow_loopback_for_tests()
    }

    #[test]
    fn rejects_metadata_ip_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let err = validate_provider_base_url("http://169.254.169.254/latest/meta-data", &policy)
            .expect_err("metadata IP");
        assert!(err.to_string().contains("169.254"));
    }

    #[test]
    fn rejects_rfc1918_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("http://10.0.0.1/v1", &policy).is_err());
        assert!(validate_provider_base_url("http://192.168.1.1/v1", &policy).is_err());
        assert!(validate_provider_base_url("http://172.16.0.1/v1", &policy).is_err());
    }

    #[test]
    fn rejects_internal_docker_hostname() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("http://postgres:5432", &policy).is_err());
        assert!(validate_provider_base_url("http://gateway:8080/v1", &policy).is_err());
    }

    #[test]
    fn rejects_non_http_scheme() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("ftp://example.com/v1", &policy).is_err());
    }

    #[test]
    fn allows_public_https() {
        let policy = UpstreamUrlPolicy::default();
        validate_provider_base_url("https://api.openai.com/v1", &policy).expect("public https");
        validate_provider_base_url("https://api.anthropic.com", &policy).expect("anthropic");
    }

    #[test]
    fn allows_loopback_when_explicitly_allowlisted() {
        let policy = policy();
        validate_provider_base_url("http://127.0.0.1:11434/v1", &policy).expect("loopback");
        validate_provider_base_url("http://localhost:11434/v1", &policy).expect("localhost");
    }

    #[tokio::test]
    async fn rebinding_blocks_private_resolution_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let err = validate_outbound_url("http://127.0.0.1:9/chat/completions", &policy)
            .await
            .expect_err("loopback blocked");
        assert!(err.to_string().contains("private") || err.to_string().contains("link-local"));
    }

    #[tokio::test]
    async fn rebinding_allows_allowlisted_loopback() {
        let policy = policy();
        validate_outbound_url("http://127.0.0.1:9/chat/completions", &policy)
            .await
            .expect("allowlisted loopback");
    }
}

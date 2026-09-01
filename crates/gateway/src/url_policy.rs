//! SSRF controls for provider `base_url` values and outbound upstream requests.
//!
//! Default-deny **allowlist** model: only public-unicast HTTPS destinations unless a
//! hostname is explicitly listed in `allow_private_upstream_hosts`. Outbound requests
//! resolve DNS once, pin the connect IP, and disable HTTP redirects.

use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use url::Url;

/// Policy for narrowly scoped private/loopback upstream opt-in.
#[derive(Debug, Clone, Default)]
pub struct UpstreamUrlPolicy {
    /// Hostnames or IP literals permitted to target non-public space (local Ollama, etc.).
    pub allow_private_hosts: HashSet<String>,
}

/// Resolved upstream target with a pinned connect address.
#[derive(Debug, Clone)]
pub struct PinTarget {
    pub url: Url,
    pub socket_addr: SocketAddr,
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

/// Validate a provider `base_url` before persisting it (hostname / literal checks only).
pub fn validate_provider_base_url(url: &str, policy: &UpstreamUrlPolicy) -> AppResult<()> {
    let parsed = parse_http_url(url)?;
    validate_url_policy(&parsed, policy)
}

/// Resolve DNS once, classify addresses, and return a pinned connect target.
pub async fn resolve_pin(url: &str, policy: &UpstreamUrlPolicy) -> AppResult<PinTarget> {
    let parsed = parse_http_url(url)?;
    validate_url_policy(&parsed, policy)?;

    let host = parsed.host_str().expect("host");
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| AppError::BadRequest("upstream URL is missing a port".into()))?;

    if let Some(ip) = parse_ip_literal(host) {
        ensure_allowed_ip(ip, host, policy)?;
        return Ok(PinTarget {
            url: parsed,
            socket_addr: SocketAddr::new(ip, port),
        });
    }

    if policy.is_private_host_allowed(host) {
        let addrs = resolve_host(host, port).await?;
        let socket_addr = addrs.into_iter().next().ok_or_else(|| {
            AppError::BadRequest(format!("upstream host '{host}' did not resolve to any address"))
        })?;
        return Ok(PinTarget {
            url: parsed,
            socket_addr,
        });
    }

    let addrs = resolve_host(host, port).await?;
    let socket_addr = pick_public_pin(host, &addrs)?;
    Ok(PinTarget {
        url: parsed,
        socket_addr,
    })
}

fn validate_url_policy(url: &Url, policy: &UpstreamUrlPolicy) -> AppResult<()> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::BadRequest("provider base_url must include a host".into()))?;

    if host_has_credentials(url) {
        return Err(AppError::BadRequest(
            "provider base_url must not include username or password".into(),
        ));
    }

    if policy.is_private_host_allowed(host) {
        return Ok(());
    }

    if let Some(ip) = parse_ip_literal(host) {
        ensure_allowed_ip(ip, host, policy)?;
        require_https(url)?;
        return Ok(());
    }

    require_https(url)?;
    ensure_public_hostname(host)?;
    Ok(())
}

fn require_https(url: &Url) -> AppResult<()> {
    if url.scheme() == "https" {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "provider base_url must use https for public upstream hosts (http is only allowed for explicitly allowlisted private hosts)".into(),
        ))
    }
}

fn ensure_public_hostname(host: &str) -> AppResult<()> {
    let host = normalize_host(host);
    if host.is_empty() {
        return Err(AppError::BadRequest(
            "provider base_url host is not allowed".into(),
        ));
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
        return Err(AppError::BadRequest(format!(
            "provider base_url host '{host}' is not allowed"
        )));
    }

    if host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".svc")
        || host.ends_with(".svc.cluster.local")
        || host.ends_with(".cluster.local")
    {
        return Err(AppError::BadRequest(format!(
            "provider base_url host '{host}' is not allowed"
        )));
    }

    if !host.contains('.') {
        return Err(AppError::BadRequest(format!(
            "provider base_url host '{host}' must be a fully qualified public hostname"
        )));
    }

    Ok(())
}

fn ensure_allowed_ip(ip: IpAddr, host: &str, policy: &UpstreamUrlPolicy) -> AppResult<()> {
    let ip = canonicalize_ip(ip);
    if policy.is_private_host_allowed(host) || is_public_unicast_ip(ip) {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "provider base_url must not target non-public address {ip}"
        )))
    }
}

fn pick_public_pin(host: &str, addrs: &[SocketAddr]) -> AppResult<SocketAddr> {
    if addrs.is_empty() {
        return Err(AppError::BadRequest(format!(
            "upstream host '{host}' did not resolve to any address"
        )));
    }

    let mut non_public = Vec::new();
    for addr in addrs {
        let ip = canonicalize_ip(addr.ip());
        if is_public_unicast_ip(ip) {
            return Ok(SocketAddr::new(ip, addr.port()));
        }
        non_public.push(ip);
    }

    Err(AppError::BadRequest(format!(
        "upstream host '{host}' resolved only to non-public address(es): {}",
        non_public
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

async fn resolve_host(host: &str, port: u16) -> AppResult<Vec<SocketAddr>> {
    #[cfg(test)]
    if let Some(addrs) = test_dns_resolve(host, port).await? {
        return Ok(addrs);
    }

    tokio::net::lookup_host((host, port))
        .await
        .map_err(|err| {
            AppError::BadRequest(format!("upstream host '{host}' could not be resolved: {err}"))
        })
        .map(|iter| iter.collect())
}

#[cfg(test)]
type TestDnsHook = std::sync::Arc<dyn Fn(&str, u16) -> AppResult<Vec<SocketAddr>> + Send + Sync>;

#[cfg(test)]
static TEST_DNS: std::sync::Mutex<Option<TestDnsHook>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn set_test_dns_hook(hook: TestDnsHook) {
    *TEST_DNS.lock().expect("test dns lock") = Some(hook);
}

#[cfg(test)]
fn clear_test_dns_hook() {
    *TEST_DNS.lock().expect("test dns lock") = None;
}

#[cfg(test)]
struct TestDnsGuard;

#[cfg(test)]
impl Drop for TestDnsGuard {
    fn drop(&mut self) {
        clear_test_dns_hook();
    }
}

#[cfg(test)]
fn test_dns_scope(hook: TestDnsHook) -> TestDnsGuard {
    set_test_dns_hook(hook);
    TestDnsGuard
}

#[cfg(test)]
async fn test_dns_resolve(host: &str, port: u16) -> AppResult<Option<Vec<SocketAddr>>> {
    let hook = TEST_DNS
        .lock()
        .expect("test dns lock")
        .clone();
    if let Some(hook) = hook {
        return Ok(Some(hook(host, port)?));
    }
    Ok(None)
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
    host.parse().ok()
}

/// Unwrap IPv4-mapped, 6to4, NAT64, and IPv4-compatible IPv6 before classification.
pub fn canonicalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => extract_embedded_ipv4(v6)
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
    }
}

/// Extract an embedded IPv4 address from transitional IPv6 forms.
pub fn extract_embedded_ipv4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = v6.to_ipv4_mapped() {
        return Some(v4);
    }

    let segments = v6.segments();

    // NAT64 well-known prefix 64:ff9b::/96
    if segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..6].iter().all(|&s| s == 0) {
        return ipv4_from_tail_hextets(segments[6], segments[7]);
    }

    // NAT64 local-use prefix 64:ff9b:1::/48 (IPv4 in low 32 bits)
    if segments[0] == 0x0064
        && segments[1] == 0xff9b
        && segments[2] == 0x0001
        && segments[3..6].iter().all(|&s| s == 0)
    {
        return ipv4_from_tail_hextets(segments[6], segments[7]);
    }

    // 6to4 2002::/16
    if segments[0] == 0x2002 {
        return ipv4_from_hextets(segments[1], segments[2]);
    }

    // IPv4-compatible ::/96 (excluding :: and ::1, excluding ::ffff: handled above)
    if segments[..6].iter().all(|&s| s == 0) && (segments[6] != 0 || segments[7] != 0) {
        return ipv4_from_tail_hextets(segments[6], segments[7]);
    }

    None
}

fn ipv4_from_hextets(high: u16, low: u16) -> Option<Ipv4Addr> {
    Some(Ipv4Addr::new(
        (high >> 8) as u8,
        (high & 0xff) as u8,
        (low >> 8) as u8,
        (low & 0xff) as u8,
    ))
}

fn ipv4_from_tail_hextets(high: u16, low: u16) -> Option<Ipv4Addr> {
    ipv4_from_hextets(high, low)
}

pub fn is_public_unicast_ip(ip: IpAddr) -> bool {
    let ip = canonicalize_ip(ip);
    match ip {
        IpAddr::V4(v4) => is_public_unicast_ipv4(v4),
        IpAddr::V6(v6) => is_public_unicast_ipv6(v6),
    }
}

fn is_public_unicast_ipv4(ip: Ipv4Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.octets()[0] == 0
        || metadata_ipv4(ip)
        || cgnat_ipv4(ip))
}

fn is_public_unicast_ipv6(ip: Ipv6Addr) -> bool {
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || documentation_ipv6(ip))
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
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn policy() -> UpstreamUrlPolicy {
        UpstreamUrlPolicy::allow_loopback_for_tests()
    }

    #[test]
    fn example_config_does_not_allow_private_hosts_by_default() {
        let yaml = include_str!("../../../config.example.yaml");
        let config: crate::config::Config = serde_yaml::from_str(yaml).expect("example yaml");
        assert!(
            config.allow_private_upstream_hosts.is_empty(),
            "config.example.yaml must not enable private upstream hosts by default"
        );
    }

    #[test]
    fn rejects_http_for_public_hostnames() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("http://api.openai.com/v1", &policy).is_err());
        validate_provider_base_url("https://api.openai.com/v1", &policy).expect("https public");
    }

    #[test]
    fn rejects_metadata_ip_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let err = validate_provider_base_url("https://169.254.169.254/latest/meta-data", &policy)
            .expect_err("metadata IP");
        assert!(err.to_string().contains("169.254"));
    }

    #[test]
    fn rejects_ipv4_mapped_loopback_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001));
        assert!(!is_public_unicast_ip(mapped));
        let err = validate_provider_base_url("https://[::ffff:127.0.0.1]/v1", &policy)
            .expect_err("mapped loopback");
        assert!(err.to_string().contains("127.0.0.1") || err.to_string().contains("non-public"));
    }

    #[test]
    fn canonicalize_unwraps_ipv4_mapped() {
        let mapped = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 0x0001));
        assert_eq!(canonicalize_ip(mapped), IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    }

    #[test]
    fn rejects_rfc1918_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("https://10.0.0.1/v1", &policy).is_err());
        assert!(validate_provider_base_url("https://192.168.1.1/v1", &policy).is_err());
        assert!(validate_provider_base_url("https://172.16.0.1/v1", &policy).is_err());
    }

    #[test]
    fn rejects_internal_docker_hostname() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("https://postgres:5432", &policy).is_err());
        assert!(validate_provider_base_url("https://gateway:8080/v1", &policy).is_err());
    }

    #[test]
    fn rejects_non_http_scheme() {
        let policy = UpstreamUrlPolicy::default();
        assert!(validate_provider_base_url("ftp://example.com/v1", &policy).is_err());
    }

    #[test]
    fn allows_loopback_when_explicitly_allowlisted() {
        let policy = policy();
        validate_provider_base_url("http://127.0.0.1:11434/v1", &policy).expect("loopback");
        validate_provider_base_url("http://localhost:11434/v1", &policy).expect("localhost");
    }

    #[test]
    fn rejects_6to4_loopback_embedding_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let addr = Ipv6Addr::new(0x2002, 0x7f00, 0x0001, 0, 0, 0, 0, 0);
        assert!(!is_public_unicast_ip(IpAddr::V6(addr)));
        assert!(validate_provider_base_url(&format!("https://[{addr}]/v1"), &policy).is_err());
    }

    #[test]
    fn rejects_6to4_rfc1918_embedding_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let addr = Ipv6Addr::new(0x2002, 0x0a00, 0, 0, 0, 0, 0, 0);
        assert!(!is_public_unicast_ip(IpAddr::V6(addr)));
        assert!(validate_provider_base_url(&format!("https://[{addr}]/v1"), &policy).is_err());
    }

    #[test]
    fn rejects_nat64_rfc1918_embedding_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let addr = Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0x0a00, 0x0001);
        assert!(!is_public_unicast_ip(IpAddr::V6(addr)));
        assert!(validate_provider_base_url(&format!("https://[{addr}]/v1"), &policy).is_err());
    }

    #[test]
    fn rejects_ipv4_compatible_loopback_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let addr = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0x7f00, 0x0001);
        assert!(!is_public_unicast_ip(IpAddr::V6(addr)));
        assert!(validate_provider_base_url(&format!("https://[{addr}]/v1"), &policy).is_err());
    }

    #[tokio::test]
    async fn resolve_pin_uses_first_public_resolution_when_dns_rebinds() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let calls = Arc::new(AtomicUsize::new(0));
        let hook = {
            let calls = calls.clone();
            Arc::new(move |_host: &str, port: u16| {
                let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if n == 1 {
                    Ok(vec![SocketAddr::from((Ipv4Addr::new(93, 184, 216, 34), port))])
                } else {
                    Ok(vec![SocketAddr::from((Ipv4Addr::new(127, 0, 0, 1), port))])
                }
            }) as TestDnsHook
        };
        let _guard = test_dns_scope(hook);

        let pin = resolve_pin("https://rebind.example/v1", &UpstreamUrlPolicy::default())
            .await
            .expect("first resolution pins public IP");
        assert_eq!(pin.socket_addr.ip(), IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_pin_blocks_loopback_without_allowlist() {
        let policy = UpstreamUrlPolicy::default();
        let err = resolve_pin("https://127.0.0.1:9/chat/completions", &policy)
            .await
            .expect_err("loopback blocked");
        assert!(err.to_string().contains("non-public"));
    }

    #[tokio::test]
    async fn resolve_pin_allows_allowlisted_loopback() {
        let policy = policy();
        resolve_pin("http://127.0.0.1:9/chat/completions", &policy)
            .await
            .expect("allowlisted loopback");
    }
}

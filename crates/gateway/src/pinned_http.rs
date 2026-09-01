//! Upstream HTTP client that pins DNS to a single connect IP and disables redirects.

use crate::error::{AppError, AppResult};
use crate::url_policy::{self, PinTarget, UpstreamUrlPolicy};
use anyhow::Context;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, RequestBuilder, Response};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static PINNED_SEND_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
pub fn pinned_send_count() -> usize {
    PINNED_SEND_COUNT.load(Ordering::SeqCst)
}

#[derive(Clone)]
struct PinResolver {
    addr: SocketAddr,
}

impl Resolve for PinResolver {
    fn resolve(&self, _name: Name) -> Resolving {
        let addr = self.addr;
        Box::pin(async move {
            let addrs: Addrs = Box::new(std::iter::once(addr));
            Ok(addrs)
        })
    }
}

fn base_builder() -> reqwest::ClientBuilder {
    Client::builder()
        .timeout(Duration::from_secs(600))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
}

pub fn http_client() -> anyhow::Result<Client> {
    base_builder()
        .build()
        .context("build HTTP client")
}

async fn pinned_client(pin: &PinTarget) -> AppResult<Client> {
    base_builder()
        .dns_resolver(Arc::new(PinResolver {
            addr: pin.socket_addr,
        }))
        .build()
        .map_err(|err| AppError::Internal(format!("build pinned HTTP client: {err}")))
}

pub async fn prepare_upstream(
    url: &str,
    policy: &UpstreamUrlPolicy,
) -> AppResult<(Client, PinTarget)> {
    let pin = url_policy::resolve_pin(url, policy).await?;
    let client = pinned_client(&pin).await?;
    Ok((client, pin))
}

pub async fn send_pinned<F>(url: &str, policy: &UpstreamUrlPolicy, build: F) -> AppResult<Response>
where
    F: FnOnce(Client, &str) -> RequestBuilder,
{
    #[cfg(test)]
    PINNED_SEND_COUNT.fetch_add(1, Ordering::SeqCst);
    let (client, pin) = prepare_upstream(url, policy).await?;
    Ok(build(client, pin.url.as_str()).send().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::url_policy::UpstreamUrlPolicy;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Simulates connect-time DNS rebind to a different address than `resolve_pin` selected.
    #[derive(Clone)]
    struct RebindEvilResolver {
        evil: SocketAddr,
    }

    impl Resolve for RebindEvilResolver {
        fn resolve(&self, _name: Name) -> Resolving {
            let evil = self.evil;
            Box::pin(async move {
                let addrs: Addrs = Box::new(std::iter::once(evil));
                Ok(addrs)
            })
        }
    }

    async fn send_with_connect_resolver<F, R>(
        url: &str,
        policy: &UpstreamUrlPolicy,
        resolver: Arc<R>,
        build: F,
    ) -> AppResult<Response>
    where
        R: Resolve + Send + Sync + 'static,
        F: FnOnce(Client, &str) -> RequestBuilder,
    {
        let pin = url_policy::resolve_pin(url, policy).await?;
        let client = base_builder()
            .dns_resolver(resolver)
            .build()
            .map_err(|err| AppError::Internal(format!("build HTTP client: {err}")))?;
        Ok(build(client, pin.url.as_str()).send().await?)
    }

    #[tokio::test]
    async fn send_pinned_connects_to_pinned_ip_when_dns_rebinds() {
        const PINNED_BODY: &str = "PINNED_OK";
        const EVIL_BODY: &str = "EVIL_REBIND";

        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/pin-check"))
            .respond_with(ResponseTemplate::new(200).set_body_string(PINNED_BODY))
            .mount(&mock)
            .await;
        let pinned_addr: SocketAddr = *mock.address();
        let pin_target = pinned_addr;
        let pinned_port = pinned_addr.port();

        let evil_listener =
            std::net::TcpListener::bind((Ipv4Addr::new(127, 0, 0, 2), pinned_port))
                .expect("bind evil rebind listener on 127.0.0.2 with pinned port");
        let evil_addr = evil_listener.local_addr().expect("evil addr");
        let evil_mock = MockServer::builder()
            .listener(evil_listener)
            .start()
            .await;
        Mock::given(method("GET"))
            .and(path("/pin-check"))
            .respond_with(ResponseTemplate::new(200).set_body_string(EVIL_BODY))
            .mount(&evil_mock)
            .await;

        let hook = Arc::new(move |host: &str, _port: u16| {
            assert_eq!(host, "rebind-pin.example");
            Ok(vec![pin_target])
        });
        let _guard = url_policy::test_dns_scope(hook);
        let policy = UpstreamUrlPolicy::from_allowlist(&["rebind-pin.example".into()]);
        let url = format!(
            "http://rebind-pin.example:{}/pin-check",
            pinned_addr.port()
        );

        let pinned_response = send_pinned(&url, &policy, |client, url| client.get(url))
            .await
            .expect("pinned HTTP connect must reach wiremock on the pinned address");
        assert_eq!(
            pinned_response.text().await.expect("pinned body"),
            PINNED_BODY,
            "provider path must connect to the address from resolve_pin, not a later DNS result"
        );

        let evil_response = send_with_connect_resolver(
            &url,
            &policy,
            Arc::new(RebindEvilResolver { evil: evil_addr }),
            |client, url| client.get(url),
        )
        .await
        .expect("rebind resolver connect");
        assert_eq!(
            evil_response.text().await.expect("evil body"),
            EVIL_BODY,
            "control: connect-time DNS rebind reaches the evil listener when PinResolver is not used"
        );
    }
}

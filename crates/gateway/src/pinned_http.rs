//! Upstream HTTP client that pins DNS to a single connect IP and disables redirects.

use crate::error::{AppError, AppResult};
use crate::url_policy::{self, PinTarget, UpstreamUrlPolicy};
use anyhow::Context;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{Client, RequestBuilder, Response};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

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
    let (client, pin) = prepare_upstream(url, policy).await?;
    Ok(build(client, pin.url.as_str()).send().await?)
}

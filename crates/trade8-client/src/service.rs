// SPDX-FileCopyrightText: Copyright Logically Magic Inc. <hello@logicallymagic.com>
// SPDX-FileCopyrightText: Copyright Hamid Jahanzad <Hamid@Jahanzad.com>
// SPDX-FileCopyrightText: Copyright Contributors to the Trade8:SDK
// SPDX-License-Identifier: Apache-2.0

//! Service-to-service caller — the Desk→Risk direct authorize call
//! (Desk→Risk authorize) and future hot-path calls (Router ack, Feed BBO read).
//!
//! Pattern: mint-or-load the CALLING service's workload certificate
//! (CERTFREE-4V — `ensure_workload_certificate`) and wrap it in the
//! mTLS transport behind `Sentin8HttpClient` (correlation-id propagation
//! + TLS context headers). The TLS handshake IS the identity — the
//! receiving service's TLS layer authenticates the certificate and the
//! enroll-published registry map authorizes the scopes; no bearer token
//! exists on this path.
//!
//! The hot path is point-to-point over Sentin8:TLS (master design §5.2)
//! — NOT bus-relayed; every accepted mutation is mirrored to the Bus
//! outbox-first by the caller.

use std::sync::Arc;

use sentin8::middleware::Sentin8HttpClient;

/// A direct service caller: certificate-presenting HTTP client.
#[derive(Clone)]
pub struct ServiceCaller {
    base_url: String,
    http: Sentin8HttpClient,
}

/// Build a certificate-presenting caller (CERTFREE-4V): mint-or-load
/// the CALLING service's own workload certificate and wrap it in the
/// mTLS transport. The workload name must appear in the calling
/// service's `M2M_GRANTS` declaration so `vertical enroll` publishes
/// its registry-map row.
///
/// - `auth_issuer` — Auth base URL.
/// - `base_url` — target service base URL (e.g. `https://risk.<env>...`).
/// - `service_name` — the CALLING service's registered workload name
///   (e.g. `trade8:desk`, `trade8:desk:feed`).
pub async fn build_caller(
    auth_issuer: &str,
    base_url: &str,
    service_name: &str,
) -> Result<ServiceCaller, String> {
    let cert = sentin8::auth::m2m::ensure_workload_certificate(
        auth_issuer,
        service_name,
        &sentin8::auth::m2m::platform_ca_path(),
    )
    .await
    .map_err(|e| format!("workload '{service_name}' certificate: {e}"))?;
    let mtls = sentin8::middleware::mtls::MtlsConfig::new(
        &cert.ca_cert_path,
        &cert.cert_path,
        &cert.key_path,
    );
    let http = sentin8::middleware::mtls::build_mtls_client(&mtls)
        .map(Sentin8HttpClient::from_client)
        .map_err(|e| format!("workload '{service_name}' transport: {e}"))?;
    Ok(ServiceCaller {
        base_url: base_url.trim_end_matches('/').to_string(),
        http,
    })
}

impl ServiceCaller {
    /// POST JSON to `path` over the certificate transport; decode the
    /// response as T.
    pub async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T, CallError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| CallError::Http(e.to_string()))?;
        resp.json::<T>()
            .await
            .map_err(|e| CallError::Decode(e.to_string()))
    }

    /// GET from `path` over the certificate transport; decode the
    /// response as T.
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, CallError> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| CallError::Http(e.to_string()))?;
        resp.json::<T>()
            .await
            .map_err(|e| CallError::Decode(e.to_string()))
    }
}

/// Call errors.
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    #[error("http: {0}")]
    Http(String),
    #[error("decode: {0}")]
    Decode(String),
}

/// Shared alias for a caller held in service state.
pub type SharedCaller = Arc<ServiceCaller>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_error_display() {
        assert_eq!(CallError::Http("h".into()).to_string(), "http: h");
    }
}

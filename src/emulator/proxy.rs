use crate::aws::client::EmulatorConnection;
use crate::settings::Settings;
use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub const MAX_ENTRIES: usize = 500;

#[derive(Clone, Debug)]
pub struct TrafficEntry {
    pub at: SystemTime,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub latency_ms: u64,
}

#[derive(Clone, Default)]
pub struct TrafficLog {
    entries: Arc<Mutex<VecDeque<TrafficEntry>>>,
}

impl TrafficLog {
    async fn push(&self, entry: TrafficEntry) {
        let mut q = self.entries.lock().await;
        if q.len() >= MAX_ENTRIES {
            q.pop_front();
        }
        q.push_back(entry);
    }

    pub async fn drain(&self) -> Vec<TrafficEntry> {
        self.entries.lock().await.drain(..).collect()
    }
}

#[derive(Default)]
pub struct ProxyManager {
    handle: Mutex<Option<JoinHandle<()>>>,
    pub log: TrafficLog,
}

impl ProxyManager {
    pub async fn start(&self, settings: &Settings, conn: &EmulatorConnection) -> Result<u16> {
        let mut handle = self.handle.lock().await;
        if handle.as_ref().is_some_and(|h| !h.is_finished()) {
            bail!("proxy is already running");
        }
        let client = conn.http_client()?;
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", settings.proxy_port))
            .await
            .with_context(|| format!("binding proxy port {}", settings.proxy_port))?;
        // Port 0 asks the OS for an ephemeral port; report the real one.
        let port = listener.local_addr().context("proxy local addr")?.port();
        let ctx = ProxyCtx {
            client,
            endpoint: conn.endpoint.clone(),
            log: self.log.clone(),
        };
        let app = Router::new().fallback(any(forward)).with_state(ctx);
        *handle = Some(tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::warn!(error = %e, "proxy server error");
            }
        }));
        tracing::info!(port, "traffic proxy started");
        Ok(port)
    }

    pub async fn stop(&self) {
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        tracing::info!("traffic proxy stopped");
    }
}

#[derive(Clone)]
struct ProxyCtx {
    client: reqwest::Client,
    endpoint: String,
    log: TrafficLog,
}

async fn forward(
    State(ctx): State<ProxyCtx>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let start = Instant::now();
    let path = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| "/".to_string());
    let target = format!("{}{}", ctx.endpoint, path);

    let mut req = ctx.client.request(method.clone(), &target);
    for (k, v) in headers.iter() {
        // Keep Host (SigV4 covers it) and content-length (body is forwarded
        // verbatim, and reqwest omits it for empty bodies, breaking signatures).
        if k == "connection" || k == "transfer-encoding" {
            continue;
        }
        req = req.header(k, v);
    }
    let result = req.body(body.to_vec()).send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) => {
            let status = resp.status();
            ctx.log
                .push(TrafficEntry {
                    at: SystemTime::now(),
                    method: method.to_string(),
                    path,
                    status: status.as_u16(),
                    latency_ms,
                })
                .await;
            let mut builder = Response::builder().status(status);
            for (k, v) in resp.headers() {
                if k == "connection" || k == "transfer-encoding" {
                    continue;
                }
                builder = builder.header(k, v);
            }
            let bytes = resp.bytes().await.unwrap_or_default();
            builder
                .body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            ctx.log
                .push(TrafficEntry {
                    at: SystemTime::now(),
                    method: method.to_string(),
                    path,
                    status: 502,
                    latency_ms,
                })
                .await;
            (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a running emulator on 127.0.0.1:9297"]
    async fn live_proxy_forwards_signed_requests() {
        let conn = EmulatorConnection::from_port(9297);
        conn.health().await.expect("emulator health");
        // Ephemeral port: never clashes with a running dashboard.
        let settings = Settings {
            proxy_port: 0,
            ..Default::default()
        };
        let proxy = ProxyManager::default();
        let port = proxy.start(&settings, &conn).await.expect("proxy start");

        let mut proxied = conn.clone();
        proxied.endpoint = format!("http://127.0.0.1:{port}");
        let config = proxied.build_aws_config().await.expect("aws config");
        let buckets = crate::aws::services::fetch_s3(&config)
            .await
            .expect("list buckets through proxy");
        let _ = buckets;

        let entries = proxy.log.drain().await;
        assert!(!entries.is_empty(), "proxy should log the request");
        assert!(entries.iter().all(|e| e.status == 200));
        proxy.stop().await;
    }

    #[tokio::test]
    #[ignore = "requires the dashboard proxy running on 127.0.0.1:9298"]
    async fn live_proxy_signature_matrix() {
        let mut conn = EmulatorConnection::from_port(9297);
        conn.endpoint = "http://127.0.0.1:9298".to_string();
        let config = conn.build_aws_config().await.expect("aws config");
        let src = std::env::temp_dir().join("proxy-matrix.txt");
        std::fs::write(&src, b"matrix").unwrap();

        let r = crate::aws::manage::list_objects(&config, "demo-bucket").await;
        println!(
            "GET non-root path: {:?}",
            r.as_ref().err().map(|e| format!("{e:#}"))
        );
        let r = crate::aws::manage::put_object(&config, "demo-bucket", "matrix.txt", &src).await;
        println!(
            "PUT non-root with body: {:?}",
            r.as_ref().err().map(|e| format!("{e:#}"))
        );
        let r = crate::aws::manage::create_bucket(&config, "proxy-matrix-bucket").await;
        println!(
            "PUT non-root empty body: {:?}",
            r.as_ref().err().map(|e| format!("{e:#}"))
        );
        let _ = crate::aws::manage::delete_object(&config, "demo-bucket", "matrix.txt").await;
        let _ = crate::aws::manage::delete_bucket(&config, "proxy-matrix-bucket").await;
    }
}

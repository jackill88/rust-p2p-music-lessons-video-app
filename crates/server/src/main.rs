use axum_server::tls_rustls::RustlsConfig;
use lesson_protocol::DEFAULT_PORT;
use lesson_server::{app, self_signed_pem, LessonStudio};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("lesson_server=info".parse().unwrap()),
        )
        .init();

    let port = std::env::var("LESSON_PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let advertised = advertised_addresses();
    let (cert_pem, key_pem) =
        self_signed_pem(&advertised).expect("generate studio TLS certificate");
    let tls = RustlsConfig::from_pem(cert_pem.into_bytes(), key_pem.into_bytes())
        .await
        .expect("load studio TLS certificate");

    let studio = Arc::new(LessonStudio::new());

    tracing::info!("Lesson Studio listening on https://{addr}");
    tracing::info!("Publish TCP port {port}; clients connect with the server IP (WSS)");
    for ip in &advertised {
        tracing::info!("Reachable at https://{ip}:{port}  (wss://{ip}:{port}/ws)");
    }

    axum_server::bind_rustls(addr, tls)
        .serve(app(studio).into_make_service())
        .await
        .unwrap();
}

fn advertised_addresses() -> Vec<IpAddr> {
    local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .map(|(_, ip)| ip)
        .filter(|ip| match ip {
            IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local(),
            IpAddr::V6(_) => false,
        })
        .collect()
}

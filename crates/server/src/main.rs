use lesson_protocol::DEFAULT_PORT;
use lesson_server::{app, LessonStudio};
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
    let studio = Arc::new(LessonStudio::new());

    tracing::info!("Lesson Studio listening on {addr}");
    tracing::info!("Publish TCP port {port} and connect clients to <server-ip>:{port}");
    for advertised in advertised_addresses() {
        tracing::info!("Reachable at {advertised}:{port}");
    }

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app(studio)).await.unwrap();
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

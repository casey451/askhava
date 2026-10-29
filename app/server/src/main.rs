//! Throwaway spike server (D-476): proves `cargo build -p server` works in `Ci`. Never merged.

#[tokio::main]
async fn main() {
    let app = core_spike::router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    drop((app, listener));
}

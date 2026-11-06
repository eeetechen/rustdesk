//! Diagnostic: exercises the real symmetric-NAT punch across an actual NAT.
//!
//! `punch_probe observe <echo>` binds a socket, asks an echo server in the middle what public
//! address it sees, and prints it. `punch_probe punch <peer-ip> <peer-anchor> <echo>` first learns
//! its own public address the same way, then runs the library's own sweep against the peer's.
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;

async fn observed_public(socket: &Arc<UdpSocket>, echo: SocketAddr) -> Option<SocketAddr> {
    let probe = b"WHOAMI";
    let mut buf = [0u8; 128];
    for _ in 0..5 {
        socket.send_to(probe, echo).await.ok()?;
        match tokio::time::timeout(Duration::from_millis(400), socket.recv_from(&mut buf)).await {
            Ok(Ok((n, _))) => {
                let text = std::str::from_utf8(&buf[..n]).ok()?.trim().to_owned();
                if let Ok(addr) = text.parse::<SocketAddr>() {
                    return Some(addr);
                }
            }
            _ => continue,
        }
    }
    None
}

#[tokio::main]
async fn main() {
    // The sweep logs what it searched and what it saw at info level, which is the point of this
    // probe: without a logger those lines go nowhere and a failed run cannot be told apart from
    // a silent one. A debug build routes them through env_logger, so RUST_LOG decides.
    hbb_common::init_log(false, "");
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let echo: SocketAddr = args.last().expect("echo addr").parse().expect("echo addr");
    // On a port-forwarding test bed the inbound half of the punch is delivered to a fixed local
    // port, so the socket has to be the one bound to it. RD_BIND_PORT names that port.
    let bind = match std::env::var("RD_BIND_PORT") {
        Ok(port) => format!("0.0.0.0:{port}"),
        Err(_) => "0.0.0.0:0".to_owned(),
    };
    let socket = Arc::new(UdpSocket::bind(&bind).await.expect("bind"));
    let mine = observed_public(&socket, echo).await;
    println!("local {} public {:?}", socket.local_addr().unwrap(), mine);
    if mode == "observe" {
        return;
    }
    let peer_ip = args[2].parse().expect("peer ip");
    let peer_anchor: u16 = args[3].parse().expect("peer anchor");
    let found =
        librustdesk::common::punch_udp_symmetric(socket.clone(), peer_ip, peer_anchor).await;
    println!("RESULT {:?}", found);
}

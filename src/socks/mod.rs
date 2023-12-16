mod server;
mod socks4;
mod socks5;

use std::net::{Ipv4Addr, Ipv6Addr};

pub use server::Server;
use tokio::net::TcpStream;

const SOCKS4: u8 = 4;
const SOCKS5: u8 = 5;

const COMMAND_CONNECT: u8 = 0x01;

type Bytes = smallvec::SmallVec<[u8; 32]>;

enum Address {
    IPv4([u8; 4]),
    IPv6([u8; 16]),
    Domain(Bytes),
}

// Request represents a request from SOCKS client.
struct Request {
    command: u8,
    address: Address,
    port: u16,
}

async fn connect_to_upstream(addr: &Address, port: u16) -> anyhow::Result<TcpStream> {
    let stream = match addr {
        Address::IPv4(ip) => TcpStream::connect((Ipv4Addr::from(*ip), port)).await,
        Address::IPv6(ip) => TcpStream::connect((Ipv6Addr::from(*ip), port)).await,
        Address::Domain(d) => {
            let s = std::str::from_utf8(&d)?;
            TcpStream::connect((s, port)).await
        }
    };
    stream.map_err(|e| anyhow::anyhow!("failed to connect to upstream: {}", e.to_string()))
}

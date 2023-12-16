mod server;
mod socks4;
mod socks5;

use std::fmt::{self, Display, Formatter};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};

pub use server::Server;
use thiserror::Error;
use tokio::net::TcpStream;

const SOCKS4: u8 = 4;
const SOCKS5: u8 = 5;

const COMMAND_CONNECT: u8 = 0x01;

type ByteBuf = smallvec::SmallVec<[u8; 32]>;

#[derive(Error, Debug)]
enum Error {
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("{0}")]
    ProtocolError(&'static str),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
enum Address {
    IPv4([u8; 4]),
    IPv6([u8; 16]),
    Domain(ByteBuf),
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Address::IPv4(i) => Ipv4Addr::from(*i).fmt(f),
            Address::IPv6(i) => Ipv6Addr::from(*i).fmt(f),
            Address::Domain(d) => String::from_utf8_lossy(&d).fmt(f),
        }
    }
}

// Request represents a request from SOCKS client.
struct Request {
    command: u8,
    address: Address,
    port: u16,
}

async fn connect_to_upstream(addr: &Address, port: u16) -> io::Result<TcpStream> {
    let stream = match addr {
        Address::IPv4(ip) => TcpStream::connect((Ipv4Addr::from(*ip), port)).await,
        Address::IPv6(ip) => TcpStream::connect((Ipv6Addr::from(*ip), port)).await,
        Address::Domain(d) => {
            let Ok(s) = std::str::from_utf8(&d) else {
                return Err(std::io::Error::other("domain name is not utf-8"));
            };
            TcpStream::connect((s, port)).await
        }
    };
    stream
}

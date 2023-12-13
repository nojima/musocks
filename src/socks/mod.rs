mod socks4;
mod server;

use std::{
    net::IpAddr,
    fmt::{Display, Formatter},
};

pub use server::Server;

#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Version {
    Socks4 = 4,
    #[allow(dead_code)]
    Socks5 = 5,
}

impl Display for Version {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match *self {
            Version::Socks4 => write!(f, "SOCKS4"),
            Version::Socks5 => write!(f, "SOCKS5"),
        }
    }
}

// Request represents a request from SOCKS client.
pub struct Request {
    pub version: Version,
    pub upstream_addr: (IpAddr, u16),
}

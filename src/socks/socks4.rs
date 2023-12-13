use std::{
    io,
    net::{Ipv4Addr, IpAddr},
};

use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt,
    },
    net::TcpStream,
};

use crate::socks::*;

pub async fn handle(
    client: &mut (impl AsyncBufRead + AsyncWrite + Unpin),
    cmd: u8,
) -> anyhow::Result<TcpStream> {
    let request = read_connect_request(client, cmd).await?;
    write_reply(client).await?;
    connect_to_upstream(request.upstream_addr).await
}

async fn read_connect_request(
    client: &mut (impl AsyncBufRead + Unpin),
    cmd: u8,
) -> anyhow::Result<Request> {
    let mut buf: [u8; 2 + 4] = Default::default();
    client.read_exact(&mut buf).await?;

    if cmd != 0x1 {
        anyhow::bail!("Only CONNECT command is supported")
    }
    let dst_port = u16::from_be_bytes([buf[0], buf[1]]);
    let dst_addr = Ipv4Addr::new(buf[2], buf[3], buf[4], buf[5]);

    let mut ident = Vec::new();
    client.read_until(0, &mut ident).await?;

    // TODO: handle socks4a

    Ok(Request {
        version: Version::Socks4,
        upstream_addr: (dst_addr.into(), dst_port),
    })
}

async fn write_reply(client: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let buf: [u8; 1 + 1 + 2 + 4] = [
        0,    // VN
        0x5A, // REP
        0, 0, // DSTPORT
        0, 0, 0, 0, // DSTIP
    ];
    client.write_all(&buf).await
}

async fn connect_to_upstream(addr: (IpAddr, u16)) -> anyhow::Result<TcpStream> {
    TcpStream::connect(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to connect to upstream: {}", e.to_string()))
}

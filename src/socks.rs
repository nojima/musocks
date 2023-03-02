use std::{io, net::Ipv4Addr};

use anyhow::bail;
use tokio::{
    io::{
        AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt,
        BufReader,
    },
    net::{TcpListener, TcpStream},
};

struct Request {
    upstream_addr: (Ipv4Addr, u16),
}

pub async fn serve() -> anyhow::Result<()> {
    let listener = TcpListener::bind("0.0.0.0:1080")
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind: {e}"))?;
    loop {
        match listener.accept().await {
            Ok((conn, _)) => {
                tokio::spawn(handle_conn(conn));
            }
            Err(err) => {
                eprintln!("ERROR: {err}");
            }
        }
    }
}

async fn handle_conn(mut client: TcpStream) -> anyhow::Result<()> {
    let mut preamble: [u8; 2] = Default::default();
    client.read_exact(&mut preamble).await?;

    let version = preamble[0];
    let upstream = match version {
        0x4 => {
            let mut reader = BufReader::new(&mut client);
            handle_socks4(&mut reader, preamble[1]).await?
        }
        _ => bail!("unsupported SOCKS version: {}", version),
    };

    let (client_reader, client_writer) = client.into_split();
    let (upstream_reader, upstream_writer) = upstream.into_split();
    do_proxy(
        client_reader,
        client_writer,
        upstream_reader,
        upstream_writer,
    )
    .await?;

    Ok(())
}

async fn handle_socks4(
    client: &mut (impl AsyncBufRead + AsyncWrite + Unpin),
    cmd: u8,
) -> anyhow::Result<TcpStream> {
    let request = read_connect_request(client, cmd).await?;
    return_reply(client).await?;
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

    Ok(Request {
        upstream_addr: (dst_addr, dst_port),
    })
}

async fn return_reply(client: &mut (impl AsyncWrite + Unpin)) -> io::Result<()> {
    let buf: [u8; 1 + 1 + 2 + 4] = [
        0,    // VN
        0x5A, // REP
        0, 0, // DSTPORT
        0, 0, 0, 0, // DSTIP
    ];
    client.write_all(&buf).await
}

async fn connect_to_upstream(addr: (Ipv4Addr, u16)) -> anyhow::Result<TcpStream> {
    TcpStream::connect(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to connect to upstream: {}", e.to_string()))
}

async fn do_proxy(
    client_reader: impl AsyncRead + Unpin,
    client_writer: impl AsyncWrite + Unpin,
    upstream_reader: impl AsyncRead + Unpin,
    upstream_writer: impl AsyncWrite + Unpin,
) -> io::Result<()> {
    tokio::try_join!(
        copy_and_drop(client_reader, upstream_writer),
        copy_and_drop(upstream_reader, client_writer),
    )?;
    Ok(())
}

async fn copy_and_drop(
    mut reader: impl AsyncRead + Unpin,
    mut writer: impl AsyncWrite + Unpin,
) -> io::Result<()> {
    tokio::io::copy(&mut reader, &mut writer).await?;
    Ok(())
}

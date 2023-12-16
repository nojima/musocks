use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::socks::*;

#[repr(u8)]
enum Status {
    Granted = 0x5a,
}

pub async fn handle(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    cmd: u8,
) -> anyhow::Result<TcpStream> {
    let request = read_connect_request(reader, cmd).await?;
    let stream = connect_to_upstream(&request.address, request.port).await?;
    write_response(writer, Status::Granted).await?;
    Ok(stream)
}

async fn read_connect_request(
    reader: &mut (impl AsyncBufRead + Unpin),
    cmd: u8,
) -> anyhow::Result<Request> {
    let mut buf = [0u8; 2 + 4];
    reader.read_exact(&mut buf).await?;

    if cmd != COMMAND_CONNECT {
        anyhow::bail!("Only CONNECT command is supported")
    }
    let dst_port = u16::from_be_bytes([buf[0], buf[1]]);
    let dst_addr = Address::IPv4([buf[2], buf[3], buf[4], buf[5]]);

    let mut ident = Vec::new();
    reader.read_until(0, &mut ident).await?;

    // TODO: handle socks4a

    Ok(Request {
        command: cmd,
        address: dst_addr,
        port: dst_port,
    })
}

async fn write_response(
    writer: &mut (impl AsyncWrite + Unpin),
    status: Status,
) -> io::Result<()> {
    #[rustfmt::skip]
    writer
        .write_all(&[
            0,            // VN
            status as u8, // REP
            0, 0,         // DSTPORT
            0, 0, 0, 0,   // DSTIP
        ])
        .await?;
    Ok(())
}

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::socks::*;

#[repr(u8)]
enum Status {
    Granted = 0x5a,
    RejectedOrFailed = 0x5b,
}

pub async fn handshake(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    cmd: u8,
) -> Result<TcpStream> {
    let request = read_request(reader, cmd).await?;
    if request.command != COMMAND_CONNECT {
        write_response(writer, Status::RejectedOrFailed).await?;
        return Err(Error::ProtocolError("command not supported"));
    }
    let upstream = match connect_to_upstream(&request.address, request.port).await {
        Ok(upstream) => upstream,
        Err(e) => {
            write_response(writer, Status::RejectedOrFailed).await?;
            return Err(Error::IoError(e));
        }
    };
    write_response(writer, Status::Granted).await?;
    Ok(upstream)
}

async fn read_request(reader: &mut (impl AsyncBufRead + Unpin), cmd: u8) -> Result<Request> {
    let dst_port = reader.read_u16().await?;

    let mut dst_addr = [0u8; 4];
    reader.read_exact(&mut dst_addr).await?;

    let mut ident = Vec::new();
    reader.read_until(0, &mut ident).await?;

    let dst_addr = if is_socks4a(dst_addr) {
        let mut domain = Vec::new();
        reader.read_until(0, &mut domain).await?;
        domain.pop(); // remove NUL
        Address::Domain(domain.into())
    } else {
        Address::IPv4(dst_addr)
    };

    Ok(Request {
        command: cmd,
        address: dst_addr,
        port: dst_port,
    })
}

async fn write_response(writer: &mut (impl AsyncWrite + Unpin), status: Status) -> io::Result<()> {
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

fn is_socks4a(dst_addr: [u8; 4]) -> bool {
    dst_addr[0] == 0 && dst_addr[1] == 0 && dst_addr[2] == 0 && dst_addr[3] != 0
}

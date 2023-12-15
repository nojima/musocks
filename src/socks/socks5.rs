use std::net::{Ipv4Addr, Ipv6Addr};

use anyhow::{bail, Ok};
use compact_str::CompactString;
use smallvec::smallvec;
use tokio::io::{AsyncBufRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

type Bytes = smallvec::SmallVec<[u8; 32]>;

pub enum Auth<'a> {
    None,
    Basic { id: &'a [u8], password: &'a [u8] },
}

pub enum AuthResult {
    Accept,
    Deny,
}

#[repr(u8)]
enum AuthMethod {
    None = 0x00,
    Basic = 0x02,
    NoAcceptableMethods = 0xff,
}

#[repr(u8)]
enum AuthStatus {
    Success = 0x00,
    Failure = 0xff,
}

#[repr(u8)]
enum Status {
    Granted = 0x00,
    CommandNotSupported = 0x07,
}

enum Address {
    IPv4([u8; 4]),
    IPv6([u8; 16]),
    Domain(CompactString),
}

struct Request {
    command: u8,
    address: Address,
    port: u16,
}

pub async fn handle(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    n_auth: u8,
) -> anyhow::Result<TcpStream> {
    negotiate_auth(reader, writer, n_auth).await?;
    let request = read_request(reader).await?;
    if request.command != 0x01 {
        write_response(writer, Status::CommandNotSupported).await?;
        bail!("command not supported: {}", request.command);
    }
    let upstream = connect_to_upstream(&request.address, request.port).await?;
    write_response(writer, Status::Granted).await?;
    Ok(upstream)
}

async fn read_available_methods(
    reader: &mut (impl AsyncBufRead + Unpin),
    n_auth: u8,
) -> anyhow::Result<Bytes> {
    let mut buf = smallvec![0u8; n_auth as usize];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_server_choice(
    writer: &mut (impl AsyncWrite + Unpin),
    chosen_auth_method: AuthMethod,
) -> anyhow::Result<()> {
    writer.write_all(&[0x05, chosen_auth_method as u8]).await?;
    Ok(())
}

async fn read_basic_auth_credential(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> anyhow::Result<(Bytes, Bytes)> {
    // read auth version
    let _ = reader.read_u8().await?;

    // read ID
    let id_len = reader.read_u8().await?;
    let mut id_buf = smallvec![0u8; id_len as usize];
    reader.read_exact(&mut id_buf).await?;

    // read password
    let password_len = reader.read_u8().await?;
    let mut password_buf = smallvec![0u8; password_len as usize];
    reader.read_exact(&mut password_buf).await?;

    Ok((id_buf, password_buf))
}

async fn write_auth_response(
    writer: &mut (impl AsyncWrite + Unpin),
    status: AuthStatus,
) -> anyhow::Result<()> {
    writer.write_all(&[0x01, status as u8]).await?;
    Ok(())
}

async fn negotiate_auth(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    n_auth: u8,
) -> anyhow::Result<()> {
    let methods = read_available_methods(reader, n_auth).await?;

    if methods.contains(&(AuthMethod::Basic as u8)) {
        write_server_choice(writer, AuthMethod::Basic).await?;
        let (id, password) = read_basic_auth_credential(reader).await?;
        match authenticate(Auth::Basic {
            id: &id,
            password: &password,
        }) {
            AuthResult::Accept => {}
            AuthResult::Deny => {
                write_auth_response(writer, AuthStatus::Failure).await?;
                bail!("authentication failure")
            }
        }
        write_auth_response(writer, AuthStatus::Success).await?;
        return Ok(());
    }

    if methods.contains(&(AuthMethod::None as u8)) {
        match authenticate(Auth::None) {
            AuthResult::Accept => {}
            AuthResult::Deny => {
                write_server_choice(writer, AuthMethod::NoAcceptableMethods).await?;
                bail!("no acceptable auth methods")
            }
        }
        write_server_choice(writer, AuthMethod::None).await?;
        return Ok(());
    }

    write_server_choice(writer, AuthMethod::NoAcceptableMethods).await?;
    bail!("no acceptable auth methods")
}

async fn read_request(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> anyhow::Result<Request> {
    let mut addr_buf = [0u8; 4];
    reader.read_exact(&mut addr_buf).await?;
    if addr_buf[0] != 0x05 {
        bail!("request is not SOCKS5");
    }
    let command = addr_buf[1];
    let address = match addr_buf[3] {
        0x01 => {
            // IPv4 address
            let mut ipv4_buf = [0u8; 4];
            reader.read_exact(&mut ipv4_buf).await?;
            Address::IPv4(ipv4_buf)
        }
        0x04 => {
            // IPv6 address
            let mut ipv6_buf = [0u8; 16];
            reader.read_exact(&mut ipv6_buf).await?;
            Address::IPv6(ipv6_buf)
        }
        0x03 => {
            // Domain name
            let len = reader.read_u8().await?;
            let mut buf: Bytes = smallvec![0u8; len as usize];
            reader.read_exact(&mut buf).await?;
            let domain = CompactString::from_utf8_lossy(&buf);
            Address::Domain(domain)
        }
        x => {
            bail!("unknown address type: {x}")
        }
    };
    let port = reader.read_u16().await?;
    Ok(Request {
        command,
        address,
        port,
    })
}

async fn write_response(
    writer: &mut (impl AsyncWrite + Unpin),
    status: Status,
) -> anyhow::Result<()> {
    #[rustfmt::skip]
    writer.write_all(&[
        0x05,                   // version
        status as u8,           // status
        0x00,                   // reserved
        0x01,                   // address type
        0x00, 0x00, 0x00, 0x00, // IPv4 address
        0x00, 0x00,             // port
    ]).await?;
    Ok(())
}

fn authenticate(auth: Auth) -> AuthResult {
    // TODO
    match auth {
        Auth::None => AuthResult::Accept,
        Auth::Basic { .. } => AuthResult::Deny,
    }
}

async fn connect_to_upstream(addr: &Address, port: u16) -> anyhow::Result<TcpStream> {
    let stream = match addr {
        Address::IPv4(ip) => TcpStream::connect((Ipv4Addr::from(*ip), port)).await,
        Address::IPv6(ip) => TcpStream::connect((Ipv6Addr::from(*ip), port)).await,
        Address::Domain(d) => TcpStream::connect((d.as_str(), port)).await,
    };
    stream.map_err(|e| anyhow::anyhow!("failed to connect to upstream: {}", e.to_string()))
}

use smallvec::smallvec;
use tokio::io::{AsyncBufRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::socks::*;

#[repr(u8)]
enum Status {
    Granted = 0x00,
    GeneralFailure = 0x01,
    NetworkUnreachable = 0x03,
    HostUnreachable = 0x04,
    ConnectionRefused = 0x05,
    CommandNotSupported = 0x07,
    AddressTypeNotSupported = 0x08,
}

pub enum Auth<'a> {
    None,
    UsernamePassword {
        username: &'a [u8],
        password: &'a [u8],
    },
}

pub enum AuthResult {
    Accept,
    Deny,
}

#[repr(u8)]
enum AuthMethod {
    None = 0x00,
    UsernamePassword = 0x02,
    NoAcceptableMethods = 0xff,
}

#[repr(u8)]
enum AuthStatus {
    Success = 0x00,
    Failure = 0xff,
}

pub async fn handshake(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    n_auth: u8,
) -> Result<(Request, TcpStream)> {
    authenticate_client(reader, writer, n_auth).await?;
    let request = read_request(reader, writer).await?;
    if request.command != COMMAND_CONNECT {
        write_response(writer, Status::CommandNotSupported).await?;
        return Err(Error::ProtocolError("command not supported"));
    }
    let upstream = match connect_to_upstream(&request.address, request.port).await {
        Ok(upstream) => upstream,
        Err(e) => {
            write_response(writer, io_error_to_status(&e)).await?;
            return Err(Error::IoError(e));
        }
    };
    write_response(writer, Status::Granted).await?;
    Ok((request, upstream))
}

async fn authenticate_client(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    n_auth: u8,
) -> Result<()> {
    let methods = read_available_methods(reader, n_auth).await?;

    if methods.contains(&(AuthMethod::UsernamePassword as u8)) {
        write_server_choice(writer, AuthMethod::UsernamePassword).await?;
        let (username, password) = read_username_and_password(reader).await?;
        match do_authenticate(Auth::UsernamePassword {
            username: &username,
            password: &password,
        }) {
            AuthResult::Accept => {}
            AuthResult::Deny => {
                write_auth_response(writer, AuthStatus::Failure).await?;
                return Err(Error::ProtocolError("authentication failure"));
            }
        }
        write_auth_response(writer, AuthStatus::Success).await?;
        return Ok(());
    }

    if methods.contains(&(AuthMethod::None as u8)) {
        match do_authenticate(Auth::None) {
            AuthResult::Accept => {}
            AuthResult::Deny => {
                write_server_choice(writer, AuthMethod::NoAcceptableMethods).await?;
                return Err(Error::ProtocolError("no acceptable auth methods"));
            }
        }
        write_server_choice(writer, AuthMethod::None).await?;
        return Ok(());
    }

    write_server_choice(writer, AuthMethod::NoAcceptableMethods).await?;
    Err(Error::ProtocolError("no acceptable auth methods"))
}

async fn read_available_methods(
    reader: &mut (impl AsyncBufRead + Unpin),
    n_auth: u8,
) -> io::Result<ByteBuf> {
    let mut buf = smallvec![0u8; n_auth as usize];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_server_choice(
    writer: &mut (impl AsyncWrite + Unpin),
    chosen_auth_method: AuthMethod,
) -> io::Result<()> {
    writer.write_all(&[0x05, chosen_auth_method as u8]).await?;
    Ok(())
}

async fn read_username_and_password(
    reader: &mut (impl AsyncBufRead + Unpin),
) -> io::Result<(ByteBuf, ByteBuf)> {
    // read auth version
    let _ = reader.read_u8().await?;

    // read username
    let username_len = reader.read_u8().await?;
    let mut username_buf = smallvec![0u8; username_len as usize];
    reader.read_exact(&mut username_buf).await?;

    // read password
    let password_len = reader.read_u8().await?;
    let mut password_buf = smallvec![0u8; password_len as usize];
    reader.read_exact(&mut password_buf).await?;

    Ok((username_buf, password_buf))
}

async fn write_auth_response(
    writer: &mut (impl AsyncWrite + Unpin),
    status: AuthStatus,
) -> io::Result<()> {
    writer.write_all(&[0x01, status as u8]).await?;
    Ok(())
}

async fn read_request(
    reader: &mut (impl AsyncBufRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
) -> Result<Request> {
    let mut addr_buf = [0u8; 4];
    reader.read_exact(&mut addr_buf).await?;
    if addr_buf[0] != 0x05 {
        return Err(Error::ProtocolError("request is not SOCKS5"));
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
            let mut buf: ByteBuf = smallvec![0u8; len as usize];
            reader.read_exact(&mut buf).await?;
            Address::Domain(buf)
        }
        _ => {
            write_response(writer, Status::AddressTypeNotSupported).await?;
            return Err(Error::ProtocolError("unknown address type"));
        }
    };
    let port = reader.read_u16().await?;
    Ok(Request {
        command,
        address,
        port,
    })
}

async fn write_response(writer: &mut (impl AsyncWrite + Unpin), status: Status) -> io::Result<()> {
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

fn do_authenticate(auth: Auth) -> AuthResult {
    // TODO
    match auth {
        Auth::None => AuthResult::Accept,
        Auth::UsernamePassword { .. } => AuthResult::Deny,
    }
}

fn io_error_to_status(e: &std::io::Error) -> Status {
    match e.raw_os_error().unwrap_or(0) {
        // ENETUNREACH
        101 => Status::NetworkUnreachable,
        // ECONNREFUSED
        111 => Status::ConnectionRefused,
        // EHOSTUNREACH
        113 => Status::HostUnreachable,
        _ => Status::GeneralFailure,
    }
}

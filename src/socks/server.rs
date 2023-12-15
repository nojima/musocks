use std::io;
use std::net::SocketAddr;
use std::time::Instant;

use anyhow::bail;
use slog::{info, o};
use tokio::io::{AsyncReadExt, AsyncWrite, BufReader, AsyncBufRead};
use tokio::net::{TcpListener, TcpStream};

use crate::socks::{socks4, socks5};

pub struct Server {
    pub logger: slog::Logger,
}

impl Server {
    pub async fn serve(self) -> anyhow::Result<()> {
        let port = 1080;
        let listener = TcpListener::bind(("0.0.0.0", port))
            .await
            .map_err(|e| anyhow::anyhow!("failed to bind: {e}"))?;
        info!(self.logger, "server started"; "port" => port);

        let mut conn_id = 0;
        loop {
            conn_id += 1;
            match listener.accept().await {
                Ok((conn, addr)) => {
                    let h = Handler {
                        logger: self.logger.new(o!("id" => conn_id)),
                    };
                    tokio::spawn(h.handle(conn, addr));
                }
                Err(err) => {
                    slog::error!(self.logger, "failed to accept"; "err" => %err);
                }
            }
        }
    }
}

struct Handler {
    logger: slog::Logger,
}

impl Handler {
    async fn handle(self, client: TcpStream, client_addr: SocketAddr) {
        if let Err(e) = self.handle_conn(client, client_addr).await {
            slog::error!(self.logger, "proxy failed"; "err" => %e);
        }
    }

    async fn handle_conn(
        &self,
        client: TcpStream,
        client_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let started_at = Instant::now();
        info!(self.logger, "proxy start"; "client_addr" => client_addr);

        let (mut client_reader, mut client_writer) = {
            let (r, w) = client.into_split();
            (BufReader::new(r), w)
        };
        let mut preamble: [u8; 2] = Default::default();
        client_reader.read_exact(&mut preamble).await?;

        let version = preamble[0];
        let upstream = match version {
            0x4 => socks4::handle(&mut client_reader, &mut client_writer, preamble[1]).await?,
            0x5 => socks5::handle(&mut client_reader, &mut client_writer, preamble[1]).await?,
            _ => bail!("unsupported SOCKS version: {}", version),
        };

        let (upstream_reader, upstream_writer) = {
            let (r, w) = upstream.into_split();
            (BufReader::new(r), w)
        };
        let (uploaded_bytes, downloaded_bytes) = self
            .do_proxy(
                client_reader,
                client_writer,
                upstream_reader,
                upstream_writer,
            )
            .await?;

        let elapsed = started_at.elapsed();
        info!(self.logger, "proxy done";
            "downloaded_bytes" => downloaded_bytes,
            "uploaded_bytes" => uploaded_bytes,
            "elapsed" => ?elapsed,
        );
        Ok(())
    }

    async fn do_proxy(
        &self,
        client_reader: impl AsyncBufRead + Unpin,
        client_writer: impl AsyncWrite + Unpin,
        upstream_reader: impl AsyncBufRead + Unpin,
        upstream_writer: impl AsyncWrite + Unpin,
    ) -> io::Result<(u64, u64)> {
        tokio::try_join!(
            self.copy_and_drop(client_reader, upstream_writer),
            self.copy_and_drop(upstream_reader, client_writer),
        )
    }

    async fn copy_and_drop(
        &self,
        mut reader: impl AsyncBufRead + Unpin,
        mut writer: impl AsyncWrite + Unpin,
    ) -> io::Result<u64> {
        let n = tokio::io::copy_buf(&mut reader, &mut writer).await?;
        drop(writer);
        drop(reader);
        Ok(n)
    }
}

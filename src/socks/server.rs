use std::{
    io,
    net::SocketAddr,
    time::Instant,
};

use anyhow::bail;
use slog::{info, o};
use tokio::{
    io::{
        AsyncRead, AsyncReadExt, AsyncWrite,
        BufReader,
    },
    net::{TcpListener, TcpStream},
};

use crate::socks::socks4;

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
        mut client: TcpStream,
        client_addr: SocketAddr,
    ) -> anyhow::Result<()> {
        let started_at = Instant::now();
        info!(self.logger, "proxy start"; "client_addr" => client_addr);

        let mut reader = BufReader::new(&mut client);
        let mut preamble: [u8; 2] = Default::default();
        reader.read_exact(&mut preamble).await?;

        let version = preamble[0];
        let upstream = match version {
            0x4 => {
                socks4::handle(&mut reader, preamble[1]).await?
            }
            _ => bail!("unsupported SOCKS version: {}", version),
        };

        let (client_reader, client_writer) = client.into_split();
        let (upstream_reader, upstream_writer) = upstream.into_split();
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
        client_reader: impl AsyncRead + Unpin,
        client_writer: impl AsyncWrite + Unpin,
        upstream_reader: impl AsyncRead + Unpin,
        upstream_writer: impl AsyncWrite + Unpin,
    ) -> io::Result<(u64, u64)> {
        tokio::try_join!(
            self.copy_and_drop(client_reader, upstream_writer),
            self.copy_and_drop(upstream_reader, client_writer),
        )
    }

    async fn copy_and_drop(
        &self,
        mut reader: impl AsyncRead + Unpin,
        mut writer: impl AsyncWrite + Unpin,
    ) -> io::Result<u64> {
        let n = tokio::io::copy(&mut reader, &mut writer).await?;
        drop(writer);
        drop(reader);
        Ok(n)
    }
}

//! Connect to remote servers/clients.

use crate::packets::configuration::{
    ClientboundConfigurationPacket, ServerboundConfigurationPacket,
};
use crate::packets::game::{ClientboundGamePacket, ServerboundGamePacket};
use crate::packets::handshaking::{ClientboundHandshakePacket, ServerboundHandshakePacket};
use crate::packets::login::clientbound_hello_packet::ClientboundHelloPacket;
use crate::packets::login::{ClientboundLoginPacket, ServerboundLoginPacket};
use crate::packets::status::{ClientboundStatusPacket, ServerboundStatusPacket};
use crate::packets::ProtocolPacket;
use crate::read::{
    deserialize_packet, read_packet_filtered, read_raw_packet, try_read_raw_packet,
    ReadPacketError,
};
use crate::write::{serialize_packet, write_raw_packet};
use azalea_auth::game_profile::GameProfile;
use azalea_auth::sessionserver::{ClientSessionServerError, ServerSessionServerError};
use azalea_crypto::{Aes128CfbDec, Aes128CfbEnc};
use bytes::BytesMut;
use std::collections::HashSet;
use std::fmt::Debug;
use std::io::Cursor;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncWriteExt, BufStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf, ReuniteError};
use tokio::net::TcpStream;
use tracing::{error, info};
use uuid::Uuid;

pub struct RawReadConnection {
    pub read_stream: OwnedReadHalf,
    pub buffer: BytesMut,
    pub compression_threshold: Option<u32>,
    pub dec_cipher: Option<Aes128CfbDec>,
}

pub struct RawWriteConnection {
    pub write_stream: OwnedWriteHalf,
    pub compression_threshold: Option<u32>,
    pub enc_cipher: Option<Aes128CfbEnc>,
}

/// The read half of a connection.
pub struct ReadConnection<R: ProtocolPacket> {
    pub raw: RawReadConnection,
    _reading: PhantomData<R>,
}

/// The write half of a connection.
pub struct WriteConnection<W: ProtocolPacket> {
    pub raw: RawWriteConnection,
    _writing: PhantomData<W>,
}

pub struct Connection<R: ProtocolPacket, W: ProtocolPacket> {
    pub reader: ReadConnection<R>,
    pub writer: WriteConnection<W>,
}

impl RawReadConnection {
    pub async fn read(&mut self) -> Result<BytesMut, Box<ReadPacketError>> {
        read_raw_packet::<_>(
            &mut self.read_stream,
            &mut self.buffer,
            self.compression_threshold,
            &mut self.dec_cipher,
        )
        .await
    }

    pub fn try_read(&mut self) -> Result<Option<BytesMut>, Box<ReadPacketError>> {
        try_read_raw_packet::<_>(
            &mut self.read_stream,
            &mut self.buffer,
            self.compression_threshold,
            &mut self.dec_cipher,
        )
    }
}

impl RawWriteConnection {
    pub async fn write(&mut self, packet: &[u8]) -> std::io::Result<()> {
        if let Err(e) = write_raw_packet(
            packet,
            &mut self.write_stream,
            self.compression_threshold,
            &mut self.enc_cipher,
        )
        .await
        {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                info!("Broken pipe, shutting down connection.");
                if let Err(e) = self.shutdown().await {
                    error!("Couldn't shut down: {}", e);
                }
            }
            return Err(e);
        }
        Ok(())
    }

    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        self.write_stream.shutdown().await
    }
}

impl<R> ReadConnection<R>
where
    R: ProtocolPacket + Debug,
{
    pub async fn read(&mut self) -> Result<R, Box<ReadPacketError>> {
        let raw_packet = self.raw.read().await?;
        deserialize_packet(&mut Cursor::new(&raw_packet[..]))
    }

    pub fn try_read(&mut self) -> Result<Option<R>, Box<ReadPacketError>> {
        let Some(raw_packet) = self.raw.try_read()? else {
            return Ok(None);
        };
        Ok(Some(deserialize_packet(&mut Cursor::new(&raw_packet[..]))?))
    }

    pub async fn read_filtered(
        &mut self,
        allowed_ids: &HashSet<u32>,
    ) -> Result<Option<R>, Box<ReadPacketError>> {
        read_packet_filtered(
            &mut self.raw.read_stream,
            &mut self.raw.buffer,
            self.raw.compression_threshold,
            &mut self.raw.dec_cipher,
            allowed_ids,
        )
        .await
    }
}

impl<W> WriteConnection<W>
where
    W: ProtocolPacket + Debug,
{
    pub async fn write(&mut self, packet: W) -> std::io::Result<()> {
        self.raw.write(&serialize_packet(&packet).unwrap()).await
    }

    pub async fn shutdown(&mut self) -> std::io::Result<()> {
        self.raw.shutdown().await
    }
}

impl<R, W> Connection<R, W>
where
    R: ProtocolPacket + Debug,
    W: ProtocolPacket + Debug,
{
    pub async fn read(&mut self) -> Result<R, Box<ReadPacketError>> {
        self.reader.read().await
    }

    pub fn try_read(&mut self) -> Result<Option<R>, Box<ReadPacketError>> {
        self.reader.try_read()
    }

    pub async fn read_filtered(
        &mut self,
        allowed_ids: &HashSet<u32>,
    ) -> Result<Option<R>, Box<ReadPacketError>> {
        self.reader.read_filtered(allowed_ids).await
    }

    pub async fn write(&mut self, packet: W) -> std::io::Result<()> {
        self.writer.write(packet).await
    }

    #[must_use]
    pub fn into_split(self) -> (ReadConnection<R>, WriteConnection<W>) {
        (self.reader, self.writer)
    }
}

#[derive(Error, Debug)]
pub enum ConnectionError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

use socks5_impl::protocol::UserKey;

#[derive(Debug, Clone)]
pub struct Proxy {
    pub addr: SocketAddr,
    pub auth: Option<UserKey>,
}

impl Proxy {
    pub fn new(addr: SocketAddr, auth: Option<UserKey>) -> Self {
        Self { addr, auth }
    }
}

impl Connection<ClientboundHandshakePacket, ServerboundHandshakePacket> {
    pub async fn new(address: &SocketAddr) -> Result<Self, ConnectionError> {
        let stream = tokio::time::timeout(
            Duration::from_secs(15),
            TcpStream::connect(address),
        )
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timed out"))??;

        stream.set_nodelay(true)?;
        Self::new_from_stream(stream).await
    }

    pub async fn new_with_proxy(
        address: &SocketAddr,
        proxy: Proxy,
    ) -> Result<Self, ConnectionError> {
        let proxy_stream = TcpStream::connect(proxy.addr).await?;
        let mut stream = BufStream::new(proxy_stream);

        let _ = socks5_impl::client::connect(&mut stream, address, proxy.auth)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        Self::new_from_stream(stream.into_inner()).await
    }

    pub async fn new_from_stream(stream: TcpStream) -> Result<Self, ConnectionError> {
        let (read_stream, write_stream) = stream.into_split();

        Ok(Connection {
            reader: ReadConnection {
                raw: RawReadConnection {
                    read_stream,
                    buffer: BytesMut::new(),
                    compression_threshold: None,
                    dec_cipher: None,
                },
                _reading: PhantomData,
            },
            writer: WriteConnection {
                raw: RawWriteConnection {
                    write_stream,
                    compression_threshold: None,
                    enc_cipher: None,
                },
                _writing: PhantomData,
            },
        })
    }

    #[must_use]
    pub fn login(self) -> Connection<ClientboundLoginPacket, ServerboundLoginPacket> {
        Connection::from(self)
    }

    #[must_use]
    pub fn status(self) -> Connection<ClientboundStatusPacket, ServerboundStatusPacket> {
        Connection::from(self)
    }
}

impl Connection<ClientboundLoginPacket, ServerboundLoginPacket> {
    pub fn set_compression_threshold(&mut self, threshold: i32) {
        if threshold >= 0 {
            self.reader.raw.compression_threshold = Some(threshold as u32);
            self.writer.raw.compression_threshold = Some(threshold as u32);
        } else {
            self.reader.raw.compression_threshold = None;
            self.writer.raw.compression_threshold = None;
        }
    }

    pub fn set_encryption_key(&mut self, key: [u8; 16]) {
        let (enc_cipher, dec_cipher) = azalea_crypto::create_cipher(&key);
        self.reader.raw.dec_cipher = Some(dec_cipher);
        self.writer.raw.enc_cipher = Some(enc_cipher);
    }

    #[must_use]
    pub fn configuration(
        self,
    ) -> Connection<ClientboundConfigurationPacket, ServerboundConfigurationPacket> {
        Connection::from(self)
    }

    pub async fn authenticate(
        &self,
        access_token: &str,
        uuid: &Uuid,
        private_key: [u8; 16],
        packet: &ClientboundHelloPacket,
    ) -> Result<(), ClientSessionServerError> {
        azalea_auth::sessionserver::join(
            access_token,
            &packet.public_key,
            &private_key,
            uuid,
            &packet.server_id,
        )
        .await
    }
}

impl Connection<ServerboundHandshakePacket, ClientboundHandshakePacket> {
    #[must_use]
    pub fn login(self) -> Connection<ServerboundLoginPacket, ClientboundLoginPacket> {
        Connection::from(self)
    }

    #[must_use]
    pub fn status(self) -> Connection<ServerboundStatusPacket, ClientboundStatusPacket> {
        Connection::from(self)
    }
}

impl Connection<ServerboundLoginPacket, ClientboundLoginPacket> {
    pub fn set_compression_threshold(&mut self, threshold: i32) {
        if threshold >= 0 {
            self.reader.raw.compression_threshold = Some(threshold as u32);
            self.writer.raw.compression_threshold = Some(threshold as u32);
        } else {
            self.reader.raw.compression_threshold = None;
            self.writer.raw.compression_threshold = None;
        }
    }

    pub fn set_encryption_key(&mut self, key: [u8; 16]) {
        let (enc_cipher, dec_cipher) = azalea_crypto::create_cipher(&key);
        self.reader.raw.dec_cipher = Some(dec_cipher);
        self.writer.raw.enc_cipher = Some(enc_cipher);
    }

    #[must_use]
    pub fn game(self) -> Connection<ServerboundGamePacket, ClientboundGamePacket> {
        Connection::from(self)
    }

    pub async fn authenticate(
        &self,
        username: &str,
        public_key: &[u8],
        private_key: &[u8; 16],
        ip: Option<&str>,
    ) -> Result<GameProfile, ServerSessionServerError> {
        azalea_auth::sessionserver::serverside_auth(username, public_key, private_key, ip).await
    }

    #[must_use]
    pub fn configuration(
        self,
    ) -> Connection<ServerboundConfigurationPacket, ClientboundConfigurationPacket> {
        Connection::from(self)
    }
}

impl Connection<ServerboundConfigurationPacket, ClientboundConfigurationPacket> {
    #[must_use]
    pub fn game(self) -> Connection<ServerboundGamePacket, ClientboundGamePacket> {
        Connection::from(self)
    }
}

impl Connection<ClientboundConfigurationPacket, ServerboundConfigurationPacket> {
    #[must_use]
    pub fn game(self) -> Connection<ClientboundGamePacket, ServerboundGamePacket> {
        Connection::from(self)
    }
}

impl Connection<ClientboundGamePacket, ServerboundGamePacket> {
    #[must_use]
    pub fn configuration(
        self,
    ) -> Connection<ClientboundConfigurationPacket, ServerboundConfigurationPacket> {
        Connection::from(self)
    }
}

impl<R1, W1> Connection<R1, W1>
where
    R1: ProtocolPacket + Debug,
    W1: ProtocolPacket + Debug,
{
    #[must_use]
    pub fn from<R2, W2>(connection: Connection<R1, W1>) -> Connection<R2, W2>
    where
        R2: ProtocolPacket + Debug,
        W2: ProtocolPacket + Debug,
    {
        Connection {
            reader: ReadConnection {
                raw: connection.reader.raw,
                _reading: PhantomData,
            },
            writer: WriteConnection {
                raw: connection.writer.raw,
                _writing: PhantomData,
            },
        }
    }

    pub fn wrap(stream: TcpStream) -> Connection<R1, W1> {
        let (read_stream, write_stream) = stream.into_split();

        Connection {
            reader: ReadConnection {
                raw: RawReadConnection {
                    read_stream,
                    buffer: BytesMut::new(),
                    compression_threshold: None,
                    dec_cipher: None,
                },
                _reading: PhantomData,
            },
            writer: WriteConnection {
                raw: RawWriteConnection {
                    write_stream,
                    compression_threshold: None,
                    enc_cipher: None,
                },
                _writing: PhantomData,
            },
        }
    }

    pub fn unwrap(self) -> Result<TcpStream, ReuniteError> {
        self.reader
            .raw
            .read_stream
            .reunite(self.writer.raw.write_stream)
    }
}

use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use tokio::net::UdpSocket;

pub(super) async fn recv(
    sock: &Option<UdpSocket>,
    buf: &mut [u8],
) -> io::Result<(usize, SocketAddr)> {
    match sock {
        Some(s) => s.recv_from(buf).await,
        None => std::future::pending().await,
    }
}

pub(super) async fn bind_v4(addr: SocketAddrV4) -> io::Result<UdpSocket> {
    UdpSocket::bind(addr).await
}

pub(super) fn bind_v6(addr: SocketAddrV6) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_only_v6(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    UdpSocket::from_std(socket.into())
}

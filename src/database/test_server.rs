//! For the database tests: a pretend server on a port of its own, playing out a conversation written by hand, and
//! the real test servers named by LIMONITER_TEST_POSTGRES and LIMONITER_TEST_MYSQL.

use super::Connection;
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;

/// Listens on a free port on this computer, and has `talk` answer the first connection.
pub fn serve<R: Send + 'static>(talk: impl FnOnce(TcpStream) -> R + Send + 'static) -> (u16, JoinHandle<R>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("its address").port();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("a connection");
        stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).expect("a timeout");
        talk(stream)
    });
    (port, handle)
}

/// A connection to a pretend server.
pub fn local(engine: super::Engine, port: u16) -> Connection {
    Connection { name: "fake".into(), host: "127.0.0.1".into(), port, user: "limtest".into(), database: "limtest".into(), tls: super::TlsMode::Disable, ..Connection::new(engine) }
}

/// The real server an environment variable names, with its password; None (and a note) when it isn't set.
pub fn real(variable: &str) -> Option<(Connection, String)> {
    let Ok(url) = std::env::var(variable) else {
        eprintln!("Skipped: set {variable} to test against a real server.");
        return None;
    };
    let (connection, password) = Connection::from_url(&url).unwrap_or_else(|| panic!("{variable} isn't a database's web address"));
    Some((connection, password.unwrap_or_default()))
}

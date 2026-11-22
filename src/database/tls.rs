//! The connection to a database server: plain TCP, or TLS over it once the server has agreed to it. PostgreSQL
//! and MySQL both start in the clear and ask for TLS partway, so a stream starts plain and is upgraded.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

/// How long to wait for a server to answer at all.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
    /// Taken away for a moment, while TLS is started on it.
    Closed,
}

impl Stream {
    /// Connects to a server, trying each address its name has in turn.
    pub fn connect(host: &str, port: u16) -> Result<Stream, String> {
        use std::net::ToSocketAddrs;
        let host_only = host.trim_matches(|c| c == '[' || c == ']');
        let addresses = (host_only, port).to_socket_addrs().map_err(|e| crate::i18n::tf!("Couldn't find {host}: {e}"))?;
        let mut last = None;
        for address in addresses {
            match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
                Ok(tcp) => {
                    let _ = tcp.set_nodelay(true);
                    return Ok(Stream::Plain(tcp));
                }
                Err(e) => last = Some(e),
            }
        }
        Err(match last {
            Some(e) => crate::i18n::tf!("Couldn't connect to {host} on port {port}: {e}"),
            None => crate::i18n::tf!("Couldn't find {host}."),
        })
    }

    pub fn is_tls(&self) -> bool {
        matches!(self, Stream::Tls(_))
    }

    /// Starts TLS on a plain connection and finishes the greeting, so a certificate that isn't accepted is found
    /// now rather than at the first query. `trust_own` accepts whatever certificate the server shows, as psql's
    /// sslmode=require does (the connection is private, but it could be someone else's server); otherwise an
    /// authority has to vouch for it, as a web browser checks.
    pub fn start_tls(self, host: &str, trust_own: bool) -> Result<Stream, String> {
        let Stream::Plain(tcp) = self else {
            return Ok(self);
        };
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let authorities = rustls::client::WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone()).build().map_err(|e| e.to_string())?;
        let verifier = Arc::new(Verifier { authorities, provider: provider.clone(), trust_own });
        let config = rustls::ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().map_err(|e| e.to_string())?.dangerous().with_custom_certificate_verifier(verifier).with_no_client_auth();
        let name = ServerName::try_from(host.trim_matches(|c| c == '[' || c == ']').to_string()).map_err(|_| crate::i18n::tf!("{host} isn't a name TLS can check a certificate against."))?;
        let connection = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
        let mut tls = Box::new(rustls::StreamOwned::new(connection, tcp));
        while tls.conn.is_handshaking() {
            if let Err(e) = tls.conn.complete_io(&mut tls.sock) {
                let about_certificate = e.get_ref().and_then(|inner| inner.downcast_ref::<rustls::Error>()).is_some_and(|e| matches!(e, rustls::Error::InvalidCertificate(_)));
                return Err(if about_certificate {
                    crate::i18n::tf!("{host}'s certificate isn't vouched for by an authority ({e}). If it's the server's own certificate, tick \u{201C}Accept the server's own certificate\u{201D} in the connection's settings.")
                } else {
                    crate::i18n::tf!("The TLS greeting with {host} failed: {e}")
                });
            }
        }
        Ok(Stream::Tls(tls))
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(tcp) => tcp.read(buf),
            Stream::Tls(tls) => tls.read(buf),
            Stream::Closed => Err(std::io::ErrorKind::NotConnected.into()),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(tcp) => tcp.write(buf),
            Stream::Tls(tls) => tls.write(buf),
            Stream::Closed => Err(std::io::ErrorKind::NotConnected.into()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(tcp) => tcp.flush(),
            Stream::Tls(tls) => tls.flush(),
            Stream::Closed => Ok(()),
        }
    }
}

/// Checks the server's certificate against the authorities, or accepts its own when asked to. Either way the
/// server has to prove it holds the certificate's key.
#[derive(Debug)]
struct Verifier {
    authorities: Arc<rustls::client::WebPkiServerVerifier>,
    provider: Arc<rustls::crypto::CryptoProvider>,
    trust_own: bool,
}

impl rustls::client::danger::ServerCertVerifier for Verifier {
    fn verify_server_cert(&self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], server_name: &ServerName<'_>, ocsp: &[u8], now: UnixTime) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if self.trust_own {
            return Ok(rustls::client::danger::ServerCertVerified::assertion());
        }
        self.authorities.verify_server_cert(end_entity, intermediates, server_name, ocsp, now)
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

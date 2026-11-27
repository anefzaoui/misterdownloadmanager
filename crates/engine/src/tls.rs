//! The certificates secure connections (HTTPS, FTPS, DNS over TLS or HTTPS) check servers with:
//! the system's, and Mozilla's ([`mozilla_roots`], the ones Firefox carries) where the system
//! lacks a server's root (#1546). A system without any (a minimal Linux without the
//! `ca-certificates` package, #264) still gives every client: plain HTTP and FTP work, and each
//! secure connection fails with [`NO_ROOTS`], which the engine reports as
//! `Error::NoCertificates`. The torrent session's HTTP clients take the same setup
//! (mdm-torrent), so a session still starts there: DHT, peers and plain HTTP trackers work,
//! HTTPS trackers fail with [`NO_ROOTS`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};
use sha2::{Digest, Sha256};

/// Why a secure connection failed on a system without certificates: the error its handshake
/// gives, in English (the command line and the event log); the interfaces word it themselves.
pub const NO_ROOTS: &str = "this system has no trusted certificates to check the server with \
                            (install the ca-certificates package)";

/// The crypto provider MDM's own TLS configurations use (the one reqwest uses too).
pub(crate) fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// Whether the system has certificates to check servers with. Windows and macOS always do
/// (their verifiers ask the system at each connection); Linux reads its CA bundle, which can be
/// missing. Asked again until it's found, so installing the certificates while MDM runs is seen
/// by the next download.
pub fn system_has_roots() -> bool {
    static FOUND: AtomicBool = AtomicBool::new(false);
    if FOUND.load(Ordering::Relaxed) {
        return true;
    }
    let found = rustls_platform_verifier::Verifier::new(provider()).is_ok();
    if found {
        FOUND.store(true, Ordering::Relaxed);
    }
    found
}

/// Mozilla's trusted root certificates (CCADB's list for websites, as Firefox carries it:
/// webpki-root-certs), which MDM's verifier trusts besides the system's (#1546). A Windows that
/// hasn't fetched a root yet (Windows downloads the roots it trusts from Windows Update the first
/// time a site needs one, and behind a proxy that often fails: Let's Encrypt's ISRG Root X1 and
/// X2 on an old Windows 10) refused such sites with "unknown issuer" while Firefox opened them.
///
/// How the system's verifier (rustls-platform-verifier) takes them: on Windows they are asked
/// only when Windows' own chain ends in a root it doesn't have or doesn't trust, and that chain
/// is then built and checked by Windows again with them (its policy, the same revocation check);
/// on macOS they are anchors beside the system's, in the same trust evaluation; on Linux they join
/// the system's CA bundle (usually Mozilla's list too) in one store. So the system's own roots
/// stay trusted (an antivirus's or a company's HTTPS inspection keeps working), and a root Mozilla
/// has is trusted even where the system's store lacks it or was told not to trust it.
///
/// A root Mozilla trusts only for some names (its name constraints: webpki-roots carries them,
/// the plain certificates here don't) is left out, so it is never trusted for more than Mozilla
/// trusts it ([`name_constrained`]).
pub fn mozilla_roots() -> impl Iterator<Item = CertificateDer<'static>> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter(|c| !name_constrained(c))
        .cloned()
}

/// Whether Mozilla's list limits the root certificate `cert` to some names: a webpki-roots
/// anchor with name constraints whose subject and public key are both `cert`'s.
fn name_constrained(cert: &[u8]) -> bool {
    let has = |part: &[u8]| !part.is_empty() && cert.windows(part.len()).any(|w| w == part);
    webpki_roots::TLS_SERVER_ROOTS.iter().any(|a| {
        a.name_constraints.is_some()
            && has(a.subject.as_ref())
            && has(a.subject_public_key_info.as_ref())
    })
}

/// [`mozilla_roots`] as reqwest takes them, made once.
fn mozilla_certificates() -> Vec<reqwest::Certificate> {
    static CERTS: std::sync::OnceLock<Vec<reqwest::Certificate>> = std::sync::OnceLock::new();
    CERTS
        .get_or_init(|| {
            mozilla_roots()
                .filter_map(|c| reqwest::Certificate::from_der(&c).ok())
                .collect()
        })
        .clone()
}

/// `builder` with the TLS it can have here: reqwest's own (the system's verifier, with
/// [`mozilla_roots`] besides) when the system has certificates, else one whose every secure
/// connection fails with [`NO_ROOTS`] (reqwest's would refuse to build the client at all, plain
/// HTTP included). MDM's own clients get it through `net::bind_client`; the torrent session's
/// (trackers, UPnP) through librqbit's `SessionOptions::configure_http_client` (mdm-torrent).
pub fn configure(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    configure_with(builder, system_has_roots(), mozilla_certificates())
}

/// `fallback`: the roots trusted besides the system's (Mozilla's; the tests give a test CA in
/// their place, a root the system doesn't have).
fn configure_with(
    builder: reqwest::ClientBuilder,
    has_roots: bool,
    fallback: Vec<reqwest::Certificate>,
) -> reqwest::ClientBuilder {
    if has_roots {
        builder.tls_certs_merge(fallback)
    } else {
        builder.tls_backend_preconfigured(no_roots_config())
    }
}

/// A client configuration for a system without certificates: every server is refused.
pub(crate) fn no_roots_config() -> rustls::ClientConfig {
    let provider = provider();
    let verifier = Arc::new(NoRoots(provider.clone()));
    rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the default protocol versions are supported")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth()
}

/// Whether `e` (or one of its causes) is a secure connection refused for want of certificates.
pub fn is_no_roots(e: &(dyn std::error::Error + 'static)) -> bool {
    let mut next = Some(e);
    while let Some(e) = next {
        if e.to_string().contains(NO_ROOTS) {
            return true;
        }
        next = e.source();
    }
    false
}

/// Why a server's certificate was refused (`Error::Certificate`): what a browser warns about
/// too. MDM has no way around it: the user is told what is wrong with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertProblem {
    /// Its validity ended.
    Expired,
    /// Its validity hasn't begun (or this computer's clock is wrong).
    NotYetValid,
    /// No certificate the system trusts vouches for it: signed by itself, or by an issuer the
    /// system doesn't know.
    Untrusted,
    /// It is for other names than the server's.
    WrongName,
    /// Its issuer took it back.
    Revoked,
    /// It isn't made for a website (its extended key usage leaves out TLS servers).
    NotForWebsites,
    /// Refused for another reason (a bad signature, an encoding).
    #[default]
    Other,
}

impl CertProblem {
    /// Whether the user may continue past this refusal (#1964): only untrusted/self-signed,
    /// expired/not-yet-valid, and wrong-name certificates may be allowed. Revoked, non-website,
    /// and other failures always stay refused.
    pub fn can_allow(self) -> bool {
        matches!(
            self,
            CertProblem::Expired
                | CertProblem::NotYetValid
                | CertProblem::Untrusted
                | CertProblem::WrongName
        )
    }
}

impl std::fmt::Display for CertProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CertProblem::Expired => "the server's certificate has expired",
            CertProblem::NotYetValid => "the server's certificate is not valid yet",
            CertProblem::Untrusted => {
                "the server's certificate is not trusted (self-signed or from an unknown issuer)"
            }
            CertProblem::WrongName => "the server's certificate is for another name",
            CertProblem::Revoked => "the server's certificate was revoked",
            CertProblem::NotForWebsites => "the server's certificate isn't made for websites",
            CertProblem::Other => "the server's certificate was refused",
        })
    }
}

/// A certificate the user chose to trust for one site (`host` and `port`, pinned to its
/// SHA-256 fingerprint and the accepted `problem`, #1964). If the server later presents another
/// certificate or fails for a different reason, MDM asks again.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AllowedCert {
    pub host: String,
    pub port: u16,
    /// Lowercase 64-character hex SHA-256 of the leaf certificate's DER bytes.
    pub sha256: String,
    /// The specific refusal reason the user accepted; if the certificate later fails for a
    /// different reason (e.g. a self-signed certificate subsequently expires), it is asked again.
    #[serde(default)]
    pub problem: CertProblem,
}

impl AllowedCert {
    /// Whether this entry matches `(host, port, sha256, problem)` (host and hex compared
    /// case-insensitively), and the problem is one that may be allowed.
    pub fn matches(&self, host: &str, port: u16, sha256: &str, problem: CertProblem) -> bool {
        problem.can_allow()
            && self.problem == problem
            && self.port == port
            && self.host.eq_ignore_ascii_case(host)
            && self.sha256.eq_ignore_ascii_case(sha256)
    }
}

/// Details of the leaf certificate a server presented when it was refused (#1964).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertInfo {
    pub host: String,
    pub port: u16,
    /// Lowercase 64-character hex SHA-256 of the leaf certificate's DER bytes.
    pub sha256: String,
    pub problem: CertProblem,
    /// The certificate's `notAfter` date (`YYYY-MM-DD`) when it has expired.
    pub expired_since: Option<String>,
    /// Whether the certificate's issuer equals its subject.
    pub self_signed: bool,
}

/// A refused server certificate (`Error::Certificate`): why it was refused and, when a TLS
/// handshake's leaf certificate was seen, the site and certificate details for prompting (#1964).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertRefusal {
    pub problem: CertProblem,
    pub info: Option<Box<CertInfo>>,
}

impl From<CertProblem> for CertRefusal {
    fn from(problem: CertProblem) -> Self {
        Self {
            problem,
            info: None,
        }
    }
}

impl std::fmt::Display for CertRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(info) = &self.info {
            match self.problem {
                CertProblem::Expired => {
                    if let Some(date) = &info.expired_since {
                        return write!(f, "the server's certificate expired on {date}");
                    }
                }
                CertProblem::Untrusted if info.self_signed => {
                    return f.write_str("the server's certificate is self-signed");
                }
                _ => {}
            }
        }
        self.problem.fmt(f)
    }
}

/// Lowercase 64-character hex SHA-256 fingerprint of a DER-encoded certificate.
pub fn sha256_hex(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Inspect a DER-encoded X.509 certificate for `(self_signed, not_after_ymd)`.
/// Pure-Rust minimal ASN.1 DER walk over `Certificate.tbsCertificate`.
pub fn inspect_cert(der: &[u8]) -> (bool, Option<String>) {
    fn read_tlv(data: &[u8]) -> Option<(u8, &[u8], &[u8])> {
        let (&tag, rest) = data.split_first()?;
        let (&len_byte, rest) = rest.split_first()?;
        let (len, rest) = if len_byte < 0x80 {
            (len_byte as usize, rest)
        } else {
            let n = (len_byte & 0x7f) as usize;
            if n == 0 || n > 4 || rest.len() < n {
                return None;
            }
            let mut len = 0usize;
            for &b in &rest[..n] {
                len = (len << 8) | (b as usize);
            }
            (len, &rest[n..])
        };
        if rest.len() < len {
            return None;
        }
        let (val, tail) = rest.split_at(len);
        Some((tag, val, tail))
    }

    fn parse_time(tag: u8, val: &[u8]) -> Option<String> {
        let s = std::str::from_utf8(val).ok()?;
        match tag {
            // UTCTime: YYMMDD...
            0x17 if s.len() >= 6 && s[..6].bytes().all(|b| b.is_ascii_digit()) => {
                let yy: u16 = s[..2].parse().ok()?;
                let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
                Some(format!("{year:04}-{}-{}", &s[2..4], &s[4..6]))
            }
            // GeneralizedTime: YYYYMMDD...
            0x18 if s.len() >= 8 && s[..8].bytes().all(|b| b.is_ascii_digit()) => {
                Some(format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..8]))
            }
            _ => None,
        }
    }

    let Some((0x30, cert_val, _)) = read_tlv(der) else {
        return (false, None);
    };
    let Some((0x30, mut tbs, _)) = read_tlv(cert_val) else {
        return (false, None);
    };
    // Optional explicit [0] version.
    if tbs.first() == Some(&0xa0) {
        let Some((_, _, rest)) = read_tlv(tbs) else {
            return (false, None);
        };
        tbs = rest;
    }
    // serialNumber, signature
    let Some((_, _, tbs)) = read_tlv(tbs) else {
        return (false, None);
    };
    let Some((_, _, tbs)) = read_tlv(tbs) else {
        return (false, None);
    };
    // issuer (raw TLV slice)
    let Some((_, issuer_val, tbs_after_issuer)) = read_tlv(tbs) else {
        return (false, None);
    };
    // validity SEQUENCE { notBefore, notAfter }
    let Some((0x30, validity_val, tbs_after_validity)) = read_tlv(tbs_after_issuer) else {
        return (false, None);
    };
    let not_after = read_tlv(validity_val)
        .and_then(|(_, _, rest)| read_tlv(rest))
        .and_then(|(tag, val, _)| parse_time(tag, val));
    // subject (raw TLV slice)
    let self_signed = read_tlv(tbs_after_validity)
        .map(|(_, subject_val, _)| !issuer_val.is_empty() && issuer_val == subject_val)
        .unwrap_or(false);
    (self_signed, not_after)
}

/// Configure `builder` for a download or probe, using the system verifier + Mozilla roots +
/// `extra_roots`, while allowing `allowed_certs` for their pinned `(host, port, sha256)` and
/// recording any refused certificate into `on_refusal`.
/// Port(s) seen for a host on one `Client` (#1964): `Single(p)` when every request to that host
/// on this client used `p`, or `Ambiguous { latest }` if a redirect hit the same host on a
/// different port (in which case pins are never matched so a wrong port never slips through).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HostPort {
    Single(u16),
    Ambiguous { latest: u16 },
}

impl HostPort {
    pub(crate) fn record(map: &mut HashMap<String, HostPort>, host: &str, port: u16) {
        let key = host.to_ascii_lowercase();
        match map.get(&key).copied() {
            None => {
                map.insert(key, HostPort::Single(port));
            }
            Some(HostPort::Single(p)) if p == port => {}
            Some(_) => {
                map.insert(key, HostPort::Ambiguous { latest: port });
            }
        }
    }
}

pub(crate) fn configure_site(
    builder: reqwest::ClientBuilder,
    extra_roots: &[Vec<u8>],
    allowed_certs: &[AllowedCert],
    ports: Arc<Mutex<HashMap<String, HostPort>>>,
    default_port: u16,
    on_refusal: Arc<dyn Fn(CertInfo) + Send + Sync>,
) -> reqwest::ClientBuilder {
    if extra_roots.is_empty() && !system_has_roots() {
        return builder.tls_backend_preconfigured(no_roots_config());
    }
    let provider = provider();
    let Ok(inner) = crate::ftp::verifier(extra_roots, provider.clone()) else {
        return configure(builder);
    };
    let verifier = Arc::new(SiteVerifier {
        inner,
        allowed: allowed_certs.to_vec(),
        ports,
        default_port,
        on_refusal,
    });
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the default protocol versions are supported")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    builder.tls_backend_preconfigured(config)
}

struct SiteVerifier {
    inner: Arc<dyn ServerCertVerifier>,
    allowed: Vec<AllowedCert>,
    ports: Arc<Mutex<HashMap<String, HostPort>>>,
    default_port: u16,
    on_refusal: Arc<dyn Fn(CertInfo) + Send + Sync>,
}

impl std::fmt::Debug for SiteVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiteVerifier")
            .field("allowed", &self.allowed.len())
            .field("default_port", &self.default_port)
            .finish_non_exhaustive()
    }
}

impl SiteVerifier {
    /// Returns `(reported_port, unambiguous)` for `host`.
    fn port_for(&self, host: &str) -> (u16, bool) {
        match self
            .ports
            .lock()
            .unwrap()
            .get(&host.to_ascii_lowercase())
            .copied()
        {
            Some(HostPort::Single(p)) => (p, true),
            Some(HostPort::Ambiguous { latest }) => (latest, false),
            None => (self.default_port, false),
        }
    }
}

impl ServerCertVerifier for SiteVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(v) => Ok(v),
            Err(err) => {
                let host = server_name.to_str().to_string();
                let (port, unambiguous) = self.port_for(&host);
                let sha256 = sha256_hex(end_entity.as_ref());
                let problem = certificate_problem(&err).unwrap_or(CertProblem::Other);
                if unambiguous
                    && problem.can_allow()
                    && self
                        .allowed
                        .iter()
                        .any(|a| a.matches(&host, port, &sha256, problem))
                {
                    return Ok(ServerCertVerified::assertion());
                }
                let (self_signed, not_after) = inspect_cert(end_entity.as_ref());
                let expired_since = (problem == CertProblem::Expired)
                    .then_some(not_after)
                    .flatten();
                (self.on_refusal)(CertInfo {
                    host,
                    port,
                    sha256,
                    problem,
                    expired_since,
                    self_signed,
                });
                Err(err)
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Whether `e` (or one of its causes) is a server's certificate refused by the verifier, and
/// why. An `io::Error` hides the error it wraps from `source()` (it gives that error's own
/// source): it is looked into as well.
pub fn certificate_problem(e: &(dyn std::error::Error + 'static)) -> Option<CertProblem> {
    use rustls::CertificateError as C;
    if let Some(rustls::Error::InvalidCertificate(c)) = e.downcast_ref::<rustls::Error>() {
        return Some(match c {
            C::Expired | C::ExpiredContext { .. } => CertProblem::Expired,
            C::NotValidYet | C::NotValidYetContext { .. } => CertProblem::NotYetValid,
            C::UnknownIssuer => CertProblem::Untrusted,
            C::NotValidForName | C::NotValidForNameContext { .. } => CertProblem::WrongName,
            C::Revoked => CertProblem::Revoked,
            C::InvalidPurpose | C::InvalidPurposeContext { .. } => CertProblem::NotForWebsites,
            C::Other(other) => of_platform(&other.to_string()),
            _ => CertProblem::Other,
        });
    }
    // What an io::Error wraps (hyper's connector wraps one io::Error in another).
    let wrapped = e
        .downcast_ref::<std::io::Error>()
        .and_then(|io| io.get_ref())
        .and_then(|inner| certificate_problem(inner));
    wrapped.or_else(|| e.source().and_then(certificate_problem))
}

/// The platform verifier's own words for what rustls has no variant for (`CertificateError::Other`).
/// On macOS (and iOS) it names only a few of the system's reasons and gives the others as
/// "<the system's description>: <its OSStatus code>", an expired certificate among them. On
/// macOS and Android a certificate not for TLS servers is its `EkuError`, by its words.
fn of_platform(said: &str) -> CertProblem {
    if said == "certificate had invalid extensions" {
        return CertProblem::NotForWebsites;
    }
    let code = said
        .rsplit_once(": ")
        .and_then(|(_, code)| code.trim().parse::<i64>().ok());
    match code {
        // errSecCertificateExpired, CSSMERR_TP_CERT_EXPIRED
        Some(-67818 | -2147409654) => CertProblem::Expired,
        // errSecCertificateNotValidYet, CSSMERR_TP_CERT_NOT_VALID_YET
        Some(-67819 | -2147409653) => CertProblem::NotYetValid,
        // errSecNotTrusted, errSecCreateChainFailed
        Some(-67843 | -25318) => CertProblem::Untrusted,
        // errSecHostNameMismatch
        Some(-67602) => CertProblem::WrongName,
        // errSecInvalidExtendedKeyUsage
        Some(-67609) => CertProblem::NotForWebsites,
        // errSecCertificateRevoked
        Some(-67820) => CertProblem::Revoked,
        _ => CertProblem::Other,
    }
}

/// The verifier of a system without certificates: no server can be checked, so none is trusted.
#[derive(Debug)]
struct NoRoots(Arc<CryptoProvider>);

fn refused() -> rustls::Error {
    rustls::Error::General(NO_ROOTS.to_owned())
}

impl ServerCertVerifier for NoRoots {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Err(refused())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(refused())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(refused())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdm_labserver::{Config, LabServer, ftp};

    /// A client of a system without certificates.
    fn without_roots() -> reqwest::Client {
        configure_with(reqwest::Client::builder(), false, Vec::new())
            .build()
            .expect("a client builds without certificates")
    }

    #[tokio::test]
    async fn without_certificates_plain_http_still_downloads() {
        let lab = LabServer::start(Config::default()).await.unwrap();
        let body = without_roots()
            .get(lab.url("/gen/16K?seed=3"))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(body.len(), 16 * 1024);
    }

    #[tokio::test]
    async fn without_certificates_https_says_why() {
        // An implicit-FTPS server opens with a TLS handshake, as an HTTPS server does: the
        // client sees its certificate and refuses it before any HTTP.
        let server = ftp::FtpServer::start(
            "127.0.0.1:0".parse().unwrap(),
            ftp::FtpConfig {
                tls: ftp::FtpTls::Implicit,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let e = without_roots()
            .get(format!("https://{}/", server.addr()))
            .send()
            .await
            .unwrap_err();
        assert!(is_no_roots(&e), "{e:?}");
        let e = crate::Error::from(e);
        assert!(matches!(e, crate::Error::NoCertificates), "{e:?}");
        assert_eq!(e.to_string(), NO_ROOTS);
    }

    /// A client of this system (its verifier, as MDM's downloads have it), trusting the lab's
    /// test CA besides.
    fn trusting(ca: Vec<u8>) -> reqwest::Client {
        let config = crate::ftp::client_config(&[ca]).unwrap();
        reqwest::Client::builder()
            .tls_backend_preconfigured(config)
            .build()
            .unwrap()
    }

    /// A refused certificate says why (#762): expired, self-signed, for another name; the
    /// system's own verifier decides (Windows', macOS's, or the CA bundle's on Linux).
    #[tokio::test]
    async fn a_refused_certificate_says_why() {
        use mdm_labserver::Cert;
        for (cert, want) in [
            (Cert::Expired, CertProblem::Expired),
            (Cert::SelfSigned, CertProblem::Untrusted),
            (Cert::WrongName, CertProblem::WrongName),
        ] {
            let lab = LabServer::start(Config {
                tls: true,
                cert,
                ..Config::default()
            })
            .await
            .unwrap();
            let url = format!("https://localhost:{}/gen/16K", lab.addr().port());
            let e = trusting(lab.ca_der().unwrap())
                .get(&url)
                .send()
                .await
                .unwrap_err();
            assert_eq!(certificate_problem(&e), Some(want), "{cert:?}: {e:?}");
            let e = crate::Error::from(e);
            assert!(
                matches!(e, crate::Error::Certificate(ref r) if r.problem == want),
                "{cert:?}: {e:?}"
            );
        }
        // The good one downloads.
        let lab = LabServer::start(Config {
            tls: true,
            ..Config::default()
        })
        .await
        .unwrap();
        let url = format!("https://localhost:{}/gen/16K", lab.addr().port());
        let body = trusting(lab.ca_der().unwrap())
            .get(&url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(body.len(), 16 * 1024);
    }

    /// A client as `configure` makes it, with `fallback` in Mozilla's roots' place.
    fn configured_with(fallback: Vec<Vec<u8>>) -> reqwest::Client {
        let fallback = fallback
            .iter()
            .map(|der| reqwest::Certificate::from_der(der).unwrap())
            .collect();
        configure_with(reqwest::Client::builder(), true, fallback)
            .build()
            .unwrap()
    }

    /// #1546: a server whose chain ends in a root the system doesn't have, but Mozilla's list
    /// does (the lab's test CA stands for it: it is in no system's store). Without the fallback
    /// the system refuses it as from an unknown issuer, as an old Windows 10 refused Let's
    /// Encrypt's; with it, the chain is accepted, and a bad certificate is still refused for
    /// what is wrong with it, by the system's own verifier (Windows' chain engine checks the
    /// chain again with the extra roots, macOS's trust evaluation, webpki on Linux).
    #[tokio::test]
    async fn a_root_only_mozilla_has_is_trusted_and_bad_certificates_still_refused() {
        use mdm_labserver::Cert;
        let good = LabServer::start(Config {
            tls: true,
            ..Config::default()
        })
        .await
        .unwrap();
        let url = format!("https://localhost:{}/gen/16K", good.addr().port());
        let ca = good.ca_der().unwrap();

        // The system alone: unknown issuer.
        let e = configured_with(Vec::new())
            .get(&url)
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            certificate_problem(&e),
            Some(CertProblem::Untrusted),
            "{e:?}"
        );

        // With the fallback's roots: it downloads.
        let body = configured_with(vec![ca])
            .get(&url)
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(body.len(), 16 * 1024);

        // Still refused, each for its own reason, with the fallback trusting the lab's CA.
        for (cert, want) in [
            (Cert::Expired, CertProblem::Expired),
            (Cert::SelfSigned, CertProblem::Untrusted),
            (Cert::WrongName, CertProblem::WrongName),
        ] {
            let lab = LabServer::start(Config {
                tls: true,
                cert,
                ..Config::default()
            })
            .await
            .unwrap();
            let url = format!("https://localhost:{}/gen/16K", lab.addr().port());
            let e = configured_with(vec![lab.ca_der().unwrap()])
                .get(&url)
                .send()
                .await
                .unwrap_err();
            assert_eq!(certificate_problem(&e), Some(want), "{cert:?}: {e:?}");
        }
    }

    /// The fallback is Mozilla's whole list, every certificate of which this system's verifier
    /// takes: one it refused would leave every client unbuilt. It holds the roots of the chain
    /// #1546's server sends (Let's Encrypt's YE1, Root YE, cross-signed by ISRG Root X2, itself
    /// by ISRG Root X1).
    #[test]
    fn the_fallback_is_mozillas_list_and_the_system_takes_it() {
        let all = webpki_root_certs::TLS_SERVER_ROOT_CERTS.len();
        assert!(all > 100, "{all}");
        // The two crates are the same list (their versions go together).
        assert_eq!(webpki_roots::TLS_SERVER_ROOTS.len(), all);
        let constrained = webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .filter(|a| a.name_constraints.is_some())
            .count();
        assert_eq!(mozilla_roots().count(), all - constrained);
        assert_eq!(mozilla_certificates().len(), all - constrained);
        for name in [&b"ISRG Root X1"[..], b"ISRG Root X2"] {
            assert!(
                mozilla_roots().any(|c| c.windows(name.len()).any(|w| w == name)),
                "{}",
                String::from_utf8_lossy(name)
            );
        }
        configure(reqwest::Client::builder())
            .build()
            .expect("the system's verifier takes Mozilla's roots");
        crate::ftp::client_config(&[]).expect("so does MDM's own TLS configuration");
    }

    /// A root Mozilla trusts only for some names is left out of the fallback: its plain
    /// certificate would be trusted for every name. Mozilla's list has one today, TUBITAK's,
    /// limited to `.tr`.
    #[test]
    fn name_constrained_roots_are_left_out() {
        let tubitak = &b"TUBITAK Kamu SM SSL Kok Sertifikasi - Surum 1"[..];
        let has = |c: &[u8], s: &[u8]| c.windows(s.len()).any(|w| w == s);
        let constrained: Vec<_> = webpki_roots::TLS_SERVER_ROOTS
            .iter()
            .filter(|a| a.name_constraints.is_some())
            .collect();
        assert!(!constrained.is_empty());
        assert!(constrained.iter().any(|a| has(a.subject.as_ref(), tubitak)));
        assert!(
            webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .any(|c| has(c, tubitak)),
            "the plain list has it"
        );
        assert!(
            mozilla_roots().all(|c| !has(&c, tubitak)),
            "the fallback doesn't"
        );
        // Each constrained anchor is exactly one certificate of the plain list, left out.
        for a in &constrained {
            let matching = webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .filter(|c| {
                    has(c, a.subject.as_ref()) && has(c, a.subject_public_key_info.as_ref())
                })
                .count();
            assert_eq!(matching, 1);
        }
        // An unconstrained root stays.
        assert!(mozilla_roots().any(|c| has(&c, b"ISRG Root X1")));
    }

    #[test]
    fn a_wrapped_certificate_error_is_found() {
        let e = std::io::Error::other(rustls::Error::InvalidCertificate(
            rustls::CertificateError::Expired,
        ));
        assert_eq!(certificate_problem(&e), Some(CertProblem::Expired));
        // As macOS's verifier gives what rustls has no variant for, and an EKU it refused.
        let other =
            |s: &str| {
                std::io::Error::other(rustls::Error::InvalidCertificate(
                    rustls::CertificateError::Other(rustls::OtherError(std::sync::Arc::from(
                        Box::<dyn std::error::Error + Send + Sync>::from(s.to_owned()),
                    ))),
                ))
            };
        for (said, want) in [
            (
                "“localhost” certificate is expired: -67818",
                CertProblem::Expired,
            ),
            (
                "“localhost” certificate is not yet valid: -67819",
                CertProblem::NotYetValid,
            ),
            (
                "“localhost” certificate is not trusted: -67843",
                CertProblem::Untrusted,
            ),
            (
                "certificate had invalid extensions",
                CertProblem::NotForWebsites,
            ),
            ("something new: -1", CertProblem::Other),
            ("no code at all", CertProblem::Other),
        ] {
            assert_eq!(certificate_problem(&other(said)), Some(want), "{said}");
        }
        let e = std::io::Error::other(rustls::Error::InvalidCertificate(
            rustls::CertificateError::InvalidPurpose,
        ));
        assert_eq!(certificate_problem(&e), Some(CertProblem::NotForWebsites));
        let e = std::io::Error::other(rustls::Error::General("handshake".into()));
        assert_eq!(certificate_problem(&e), None);
        assert_eq!(certificate_problem(&std::io::Error::other(refused())), None);
    }

    #[test]
    fn other_tls_failures_are_not_mistaken_for_it() {
        let e = std::io::Error::other(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ));
        assert!(!is_no_roots(&e));
        assert!(is_no_roots(&std::io::Error::other(refused())));
    }

    /// #1964: a refused certificate carries its site, SHA-256 fingerprint, self-signed flag, and
    /// expiry date; pinning `(host, port, sha256)` lets that site's certificate through while a
    /// different port or different certificate is still refused.
    #[tokio::test]
    async fn allowed_cert_pins_site_port_and_fingerprint() {
        use mdm_labserver::Cert;
        for (server, want_problem, want_self_signed, want_expired) in [
            (Cert::SelfSigned, CertProblem::Untrusted, true, false),
            (Cert::Expired, CertProblem::Expired, false, true),
            (Cert::WrongName, CertProblem::WrongName, false, false),
        ] {
            let lab = LabServer::start(Config {
                tls: true,
                cert: server,
                ..Config::default()
            })
            .await
            .unwrap();
            let port = lab.addr().port();
            let url = url::Url::parse(&format!("https://localhost:{port}/gen/16K")).unwrap();
            let base_opts = crate::Options {
                extra_roots: vec![lab.ca_der().unwrap()],
                ..Default::default()
            };

            let err = crate::probe(url.clone(), reqwest::header::HeaderMap::new(), &base_opts)
                .await
                .unwrap_err();
            let crate::Error::Certificate(refusal) = err else {
                panic!("{server:?}: expected Error::Certificate, got {err:?}");
            };
            assert_eq!(refusal.problem, want_problem, "{server:?}");
            let info = refusal.info.expect("refused cert info must be recorded");
            assert_eq!(info.host, "localhost");
            assert_eq!(info.port, port);
            assert_eq!(info.sha256.len(), 64);
            assert_eq!(info.self_signed, want_self_signed, "{server:?}");
            assert_eq!(info.expired_since.is_some(), want_expired, "{server:?}");

            // Wrong port is still refused.
            let mut wrong_port = base_opts.clone();
            wrong_port.allowed_certs.push(AllowedCert {
                host: info.host.clone(),
                port: port + 1,
                sha256: info.sha256.clone(),
                problem: info.problem,
            });
            assert!(
                matches!(
                    crate::probe(url.clone(), reqwest::header::HeaderMap::new(), &wrong_port).await,
                    Err(crate::Error::Certificate(_))
                ),
                "{server:?}: wrong port must still fail"
            );

            // Wrong fingerprint is still refused.
            let mut wrong_fp = base_opts.clone();
            wrong_fp.allowed_certs.push(AllowedCert {
                host: info.host.clone(),
                port,
                sha256: "0".repeat(64),
                problem: info.problem,
            });
            assert!(
                matches!(
                    crate::probe(url.clone(), reqwest::header::HeaderMap::new(), &wrong_fp).await,
                    Err(crate::Error::Certificate(_))
                ),
                "{server:?}: wrong fingerprint must still fail"
            );

            // Different problem for the same cert is still refused.
            let other_problem = if info.problem == CertProblem::Expired {
                CertProblem::Untrusted
            } else {
                CertProblem::Expired
            };
            let mut wrong_reason = base_opts.clone();
            wrong_reason.allowed_certs.push(AllowedCert {
                host: info.host.clone(),
                port,
                sha256: info.sha256.clone(),
                problem: other_problem,
            });
            assert!(
                matches!(
                    crate::probe(
                        url.clone(),
                        reqwest::header::HeaderMap::new(),
                        &wrong_reason
                    )
                    .await,
                    Err(crate::Error::Certificate(_))
                ),
                "{server:?}: different problem must still fail"
            );

            // Matching (host, port, sha256, problem) succeeds.
            let mut allowed = base_opts.clone();
            allowed.allowed_certs.push(AllowedCert {
                host: info.host,
                port,
                sha256: info.sha256,
                problem: info.problem,
            });
            let probed = crate::probe(url, reqwest::header::HeaderMap::new(), &allowed)
                .await
                .unwrap();
            assert_eq!(probed.size, Some(16 * 1024));
        }
    }

    /// #1964: only Untrusted, Expired, NotYetValid, and WrongName may be allowed; Revoked,
    /// NotForWebsites, and Other always stay refused.
    #[test]
    fn only_allowable_problems_can_be_bypassed() {
        for p in [
            CertProblem::Expired,
            CertProblem::NotYetValid,
            CertProblem::Untrusted,
            CertProblem::WrongName,
        ] {
            assert!(p.can_allow(), "{p:?}");
            let a = AllowedCert {
                host: "example.com".into(),
                port: 443,
                sha256: "aa".repeat(32),
                problem: p,
            };
            assert!(a.matches("example.com", 443, &"aa".repeat(32), p));
        }
        for p in [
            CertProblem::Revoked,
            CertProblem::NotForWebsites,
            CertProblem::Other,
        ] {
            assert!(!p.can_allow(), "{p:?}");
            let a = AllowedCert {
                host: "example.com".into(),
                port: 443,
                sha256: "aa".repeat(32),
                problem: p,
            };
            assert!(!a.matches("example.com", 443, &"aa".repeat(32), p));
        }
    }

    /// #1964: a pin for `localhost` must not allow `127.0.0.1`, and an allowed site redirecting
    /// to another host with a bad certificate must still be refused, naming the second host.
    #[tokio::test]
    async fn pin_does_not_cross_hosts_or_redirects() {
        use mdm_labserver::Cert;
        let lab1 = LabServer::start(Config {
            tls: true,
            cert: Cert::SelfSigned,
            ..Config::default()
        })
        .await
        .unwrap();
        let lab2 = LabServer::start(Config {
            tls: true,
            cert: Cert::SelfSigned,
            ..Config::default()
        })
        .await
        .unwrap();
        let p1 = lab1.addr().port();
        let p2 = lab2.addr().port();

        let err = crate::probe(
            url::Url::parse(&format!("https://localhost:{p1}/gen/16K")).unwrap(),
            reqwest::header::HeaderMap::new(),
            &crate::Options::default(),
        )
        .await
        .unwrap_err();
        let crate::Error::Certificate(refusal) = err else {
            panic!("expected certificate error, got {err:?}");
        };
        let info1 = refusal.info.unwrap();

        let opts = crate::Options {
            allowed_certs: vec![AllowedCert {
                host: "localhost".into(),
                port: p1,
                sha256: info1.sha256,
                problem: info1.problem,
            }],
            ..Default::default()
        };

        // Pin for `localhost` must not let `127.0.0.1` through.
        let ip_err = crate::probe(
            url::Url::parse(&format!("https://127.0.0.1:{p1}/gen/16K")).unwrap(),
            reqwest::header::HeaderMap::new(),
            &opts,
        )
        .await
        .unwrap_err();
        let crate::Error::Certificate(ip_refusal) = ip_err else {
            panic!("expected certificate error for 127.0.0.1, got {ip_err:?}");
        };
        assert_eq!(ip_refusal.info.unwrap().host, "127.0.0.1");

        // Allowed `localhost:<p1>` redirecting to `127.0.0.1:<p2>` must still be refused,
        // naming the second host and port.
        let target = format!("https://127.0.0.1:{p2}/gen/16K");
        let redir_url = url::Url::parse(&format!(
            "https://localhost:{p1}/redirect?to={}",
            url::form_urlencoded::byte_serialize(target.as_bytes()).collect::<String>()
        ))
        .unwrap();
        let redir_err = crate::probe(redir_url, reqwest::header::HeaderMap::new(), &opts)
            .await
            .unwrap_err();
        let crate::Error::Certificate(redir_refusal) = redir_err else {
            panic!("expected certificate error after redirect, got {redir_err:?}");
        };
        let redir_info = redir_refusal.info.unwrap();
        assert_eq!(redir_info.host, "127.0.0.1");
        assert_eq!(redir_info.port, p2);
    }

    /// #1964: redirecting to the same host on a different port tracks the port per connection,
    /// so the second port is refused with its own port and the first port stays allowed.
    #[tokio::test]
    async fn redirect_to_same_host_on_another_port_tracks_port_per_connection() {
        use mdm_labserver::Cert;
        let lab1 = LabServer::start(Config {
            tls: true,
            cert: Cert::SelfSigned,
            ..Config::default()
        })
        .await
        .unwrap();
        let lab2 = LabServer::start(Config {
            tls: true,
            cert: Cert::SelfSigned,
            ..Config::default()
        })
        .await
        .unwrap();
        let p1 = lab1.addr().port();
        let p2 = lab2.addr().port();

        let err = crate::probe(
            url::Url::parse(&format!("https://localhost:{p1}/gen/16K")).unwrap(),
            reqwest::header::HeaderMap::new(),
            &crate::Options::default(),
        )
        .await
        .unwrap_err();
        let crate::Error::Certificate(r1) = err else {
            panic!("expected certificate error, got {err:?}");
        };
        let info1 = r1.info.unwrap();

        let opts = crate::Options {
            allowed_certs: vec![AllowedCert {
                host: "localhost".into(),
                port: p1,
                sha256: info1.sha256,
                problem: info1.problem,
            }],
            ..Default::default()
        };

        let target = format!("https://localhost:{p2}/gen/16K");
        let redir_url = url::Url::parse(&format!(
            "https://localhost:{p1}/redirect?to={}",
            url::form_urlencoded::byte_serialize(target.as_bytes()).collect::<String>()
        ))
        .unwrap();
        let redir_err = crate::probe(redir_url, reqwest::header::HeaderMap::new(), &opts)
            .await
            .unwrap_err();
        let crate::Error::Certificate(redir_refusal) = redir_err else {
            panic!("expected certificate error on second port, got {redir_err:?}");
        };
        let redir_info = redir_refusal.info.unwrap();
        assert_eq!(redir_info.host, "localhost");
        assert_eq!(redir_info.port, p2);

        // First port still succeeds.
        let ok = crate::probe(
            url::Url::parse(&format!("https://localhost:{p1}/gen/16K")).unwrap(),
            reqwest::header::HeaderMap::new(),
            &opts,
        )
        .await
        .unwrap();
        assert_eq!(ok.size, Some(16 * 1024));
    }
}

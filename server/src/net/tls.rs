use std::{io::BufReader, sync::Arc};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};

use crate::protocol::ALPN;

/// Long-term TLS identity of this Aqua server.
///
/// Distinct from the per-run `ServerSessionID`. The fingerprint is the identity
/// clients pin. Phase 3A ships a development certificate; see `docs/TRANSPORT.md`.
#[derive(Clone)]
pub struct TlsIdentity {
    pub cert_der: Vec<u8>,
    pub fingerprint_sha256: String,
}

/// Build the QUIC server TLS config from PEM cert + key, plus its identity.
pub fn server_config(
    cert_pem: &[u8],
    key_pem: &[u8],
) -> Result<(Arc<rustls::ServerConfig>, TlsIdentity), String> {
    let certs = read_certs(cert_pem)?;
    let key = read_key(key_pem)?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs.clone(), key)
        .map_err(|e| e.to_string())?;
    config.alpn_protocols = vec![ALPN.to_vec()];

    let identity = TlsIdentity {
        fingerprint_sha256: sha256_hex(&certs[0]),
        cert_der: certs[0].to_vec(),
    };
    Ok((Arc::new(config), identity))
}

/// QUIC client TLS config for tests. Trusts the Aqua development certificate by
/// pinning its SHA-256 fingerprint (not "accept anything").
pub fn client_config(pinned_fingerprint: &str) -> Result<Arc<rustls::ClientConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(PinnedVerifier {
            fingerprint: pinned_fingerprint.to_ascii_lowercase(),
        }))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(Arc::new(config))
}

fn read_certs(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>, String> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

fn read_key(pem: &[u8]) -> Result<PrivateKeyDer<'static>, String> {
    let mut reader = BufReader::new(pem);
    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no private key found in PEM".to_string())
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Development verifier: accepts only a certificate whose SHA-256 fingerprint
/// matches the pinned value.
#[derive(Debug)]
struct PinnedVerifier {
    fingerprint: String,
}

impl rustls::client::danger::ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let actual = sha256_hex(end_entity);
        if actual.eq_ignore_ascii_case(&self.fingerprint) {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "certificate fingerprint mismatch: expected {}, got {actual}",
                self.fingerprint
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        let provider = rustls::crypto::ring::default_provider();
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

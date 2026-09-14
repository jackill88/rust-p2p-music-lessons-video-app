use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use std::net::{IpAddr, Ipv4Addr};

pub fn self_signed_pem(extra_ips: &[IpAddr]) -> Result<(String, String), rcgen::Error> {
    let mut params = CertificateParams::new(vec!["localhost".into(), "lesson-studio".into()])?;
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "Lesson Studio");

    params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));

    for ip in extra_ips {
        if !matches!(ip, IpAddr::V4(v4) if v4.is_loopback()) {
            params.subject_alt_names.push(SanType::IpAddress(*ip));
        }
    }

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}

//! Public URL validation for retained record matching.
use std::net::IpAddr;
use url::Url;

/// # Errors
/// Rejects non-HTTP schemes, userinfo, unusual ports, and non-public names or addresses.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // These are DNS suffixes, not file extensions.
pub fn public_url(input: &str) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| "invalid URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 80 && port != 443)
    {
        return Err(
            "only public HTTP(S) URLs without credentials and with standard ports are supported"
                .into(),
        );
    }
    let host = url.host_str().ok_or("URL has no host")?;
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || !host.contains('.') && !host.contains(':')
    {
        return Err("non-public hostname is unsupported".into());
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>()
        && !public_ip(ip)
    {
        return Err("non-public IP address is unsupported".into());
    }
    Ok(url)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.is_multicast()
                || a == 0
                || a >= 240
                || a == 100 && (64..=127).contains(&b)
                || a == 198 && matches!(b, 18 | 19)
                || a == 192 && b == 0 && c == 0)
        }
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(ip));
            }
            let segments = ip.segments();
            // Only ordinary global unicast. Excludes local, multicast, link-local,
            // documentation and transition ranges that can embed private IPv4.
            (segments[0] & 0xe000) == 0x2000
                && !(segments[0] == 0x2001 && (segments[1] == 0xdb8 || segments[1] < 0x0200))
                && segments[0] != 0x2002
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn public_network_only() {
        for url in [
            "http://127.0.0.1",
            "http://[::1]",
            "http://[::ffff:127.0.0.1]",
            "http://169.254.169.254",
            "http://10.1.1.1",
            "http://100.64.0.1",
            "http://localhost",
            "file:///tmp/jobs",
            "https://user:secret@example.com",
            "https://example.com:8000",
        ] {
            assert!(public_url(url).is_err(), "{url}");
        }
        assert!(public_url("https://boards.greenhouse.io/acme").is_ok());
        assert!(public_ip("8.8.8.8".parse().unwrap()));
    }
}

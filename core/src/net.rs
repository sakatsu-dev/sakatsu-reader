//! Whether an address is this computer's own.
//!
//! Used to describe where data goes: a recognition endpoint on loopback keeps the image on this
//! machine; anything else sends it somewhere else. **Private-network addresses count as
//! elsewhere.** `192.168.1.10` is another computer, and saying "local" about it would tell a
//! reader their image stays on their machine when it does not.

use std::net::IpAddr;

/// True only for `localhost`, `127.0.0.0/8` and `::1`. Anything unparseable is not loopback.
pub fn is_loopback(url: &str) -> bool {
    let Ok(uri) = url.trim().parse::<ureq::http::Uri>() else {
        return false;
    };
    let Some(host) = uri.host() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_addresses_are_local() {
        for url in [
            "http://127.0.0.1:8140",
            "http://127.4.5.6/v1",
            "http://localhost:8080",
            "http://LOCALHOST",
            "http://[::1]:8000/v1",
        ] {
            assert!(is_loopback(url), "{url} is this computer");
        }
    }

    #[test]
    fn everything_else_is_elsewhere_including_private_networks() {
        for url in [
            "http://192.168.1.10:8140",
            "http://10.0.0.2",
            "http://172.16.0.1",
            "https://api.example.com/v1",
            "http://localhost.example.com",
            "http://127.0.0.1.example.com",
            "",
            "not a url",
        ] {
            assert!(!is_loopback(url), "{url} is not this computer");
        }
    }
}

//! URL formatting helpers for remote provider links.

/// Whether `host` contains only characters accepted in a URL authority.
pub fn safe_url_host(host: &str) -> bool {
    !host.is_empty()
        && host.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
}

/// Whether `authority` contains only characters accepted in a URL authority.
pub fn safe_url_authority(authority: &str) -> bool {
    !authority.is_empty()
        && authority.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
        })
}

/// Whether `bucket` is safe to use as an S3 URL authority.
pub fn safe_s3_bucket(bucket: &str) -> bool {
    !bucket.is_empty()
        && bucket.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        })
}

/// Whether `path` is a well-formed URI path without unsafe bytes.
pub fn safe_uri_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
            continue;
        }
        if !(byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'/' | b':'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'-'
                    | b'.'
                    | b'_'
                    | b'~'
            ))
        {
            return false;
        }
        index += 1;
    }
    true
}

/// Percent-encodes URL path components while preserving separators.
pub fn encode_url_path(path: &str) -> String {
    path.split('/')
        .map(encode_url_component)
        .collect::<Vec<_>>()
        .join("/")
}

/// Percent-encodes one URL component.
pub fn encode_url_component(component: &str) -> String {
    let mut encoded = String::new();
    for byte in component.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

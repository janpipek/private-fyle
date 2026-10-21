//! Reversible escaping for SFTP strings that contain non-UTF-8 filename bytes.

/// Encodes bytes as UTF-8 text while preserving valid UTF-8 and escaping tildes
/// and invalid bytes so a later [`decode`] can reconstruct the exact byte sequence.
pub fn encode(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len());
    let mut offset = 0;

    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(valid) => {
                push_valid_text(&mut encoded, valid);
                break;
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                let valid_end = offset + valid_len;
                if valid_len > 0 {
                    // The prefix was validated by `from_utf8` above.
                    let valid = std::str::from_utf8(&bytes[offset..valid_end])
                        .expect("validated UTF-8 prefix");
                    push_valid_text(&mut encoded, valid);
                    offset = valid_end;
                }

                let invalid_len = error.error_len().unwrap_or(bytes.len() - offset);
                for byte in &bytes[offset..offset + invalid_len] {
                    push_escape(&mut encoded, *byte);
                }
                offset += invalid_len;
            }
        }
    }

    encoded
}

/// Decodes only recognized escapes, preserving all other text and low bytes.
pub fn decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut offset = 0;

    while offset < bytes.len() {
        if let Some(byte) = recognized_escape(&bytes[offset..]) {
            decoded.push(byte);
            offset += 4;
        } else {
            decoded.push(bytes[offset]);
            offset += 1;
        }
    }

    decoded
}

fn push_valid_text(output: &mut String, text: &str) {
    for character in text.chars() {
        if character == '~' {
            push_escape(output, b'~');
        } else {
            output.push(character);
        }
    }
}

fn push_escape(output: &mut String, byte: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    output.push_str("~x");
    output.push(HEX[(byte >> 4) as usize] as char);
    output.push(HEX[(byte & 0x0f) as usize] as char);
}

fn recognized_escape(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < 4 || bytes[0] != b'~' || bytes[1] != b'x' {
        return None;
    }

    let byte = (hex_value(bytes[2])? << 4) | hex_value(bytes[3])?;
    (byte == b'~' || byte >= 0x80).then_some(byte)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use serde::{Deserialize, Serialize};

    use super::{decode, encode};

    #[test]
    fn encoding_is_injective_for_marker_like_and_invalid_bytes() {
        let raw_names: [&[u8]; 5] = [b"~x80", b"~x7E", b"name\x80", b"name\x81", b"cafe\xc3\xa9"];
        let encoded = raw_names.map(encode);

        for (raw, text) in raw_names.into_iter().zip(&encoded) {
            assert_eq!(decode(text), raw);
        }
        for left in 0..encoded.len() {
            for right in left + 1..encoded.len() {
                assert_ne!(encoded[left], encoded[right]);
            }
        }
        assert_eq!(encoded[0], "~x7Ex80");
        assert_eq!(encoded[2], "name~x80");
        assert_eq!(encoded[3], "name~x81");
    }

    #[test]
    fn decoding_does_not_turn_escapes_into_path_structure() {
        assert_eq!(decode("a/b/../.~x2F~x00"), b"a/b/../.~x2F~x00");
        assert_eq!(decode("~x7E~x80~xFF~x7e"), b"~\x80\xff~x7e");
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct Handle {
        id: u32,
        handle: String,
    }

    #[derive(Debug, Deserialize, Serialize)]
    struct Data {
        id: u32,
        #[serde(with = "serde_bytes")]
        data: Vec<u8>,
    }

    #[cfg(feature = "lossless-strings")]
    #[test]
    fn serde_round_trips_binary_handle_and_leaves_file_data_unchanged() {
        let raw_handle = b"h\x00\x80~xFF";
        let handle = Handle {
            id: 17,
            handle: encode(raw_handle),
        };
        let wire = crate::ser::to_bytes(&handle).expect("serialize handle");
        let mut expected = Vec::new();
        expected.extend_from_slice(&17u32.to_be_bytes());
        expected.extend_from_slice(&(raw_handle.len() as u32).to_be_bytes());
        expected.extend_from_slice(raw_handle);
        assert_eq!(wire.as_ref(), expected);

        let mut input = Bytes::copy_from_slice(&wire);
        let decoded: Handle = crate::de::from_bytes(&mut input).expect("deserialize handle");
        assert_eq!(decode(&decoded.handle), raw_handle);

        let payload = vec![0, b'~', b'x', b'8', b'0', 0x80, 0xff];
        let data_wire = crate::ser::to_bytes(&Data {
            id: 19,
            data: payload.clone(),
        })
        .expect("serialize data");
        assert_eq!(&data_wire[8..], payload.as_slice());

        let mut data_input = Bytes::copy_from_slice(&data_wire);
        let data: Data = crate::de::from_bytes(&mut data_input).expect("deserialize data");
        assert_eq!(data.data, payload);
    }
}

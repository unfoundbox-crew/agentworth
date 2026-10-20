//! DSSE v1 pre-authentication encoding.

/// `PAE(type, body) = "DSSEv1" SP LEN(type) SP type SP LEN(body) SP body`, where `LEN` is the
/// ASCII decimal byte length with no leading zeros (DSSE protocol.md, "Signature Definition").
pub fn pae(payload_type: &str, body: &[u8]) -> Vec<u8> {
    let t = payload_type.as_bytes();
    let mut out = Vec::with_capacity(32 + t.len() + body.len());
    out.extend_from_slice(b"DSSEv1 ");
    out.extend_from_slice(t.len().to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(t);
    out.push(b' ');
    out.extend_from_slice(body.len().to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test vector from the DSSE spec, protocol.md v1.0.2 "Test Vectors"
    /// (https://github.com/secure-systems-lab/dsse/blob/master/protocol.md), produced by the
    /// reference implementation `implementation/signing_spec.py`:
    /// PAYLOAD_TYPE `http://example.com/HelloWorld`, SERIALIZED_BODY `hello world`,
    /// PAE `DSSEv1 29 http://example.com/HelloWorld 11 hello world`.
    #[test]
    fn dsse_spec_hello_world_vector() {
        assert_eq!(
            pae("http://example.com/HelloWorld", b"hello world"),
            b"DSSEv1 29 http://example.com/HelloWorld 11 hello world".to_vec()
        );
    }

    #[test]
    fn empty_type_and_body_have_zero_lengths_without_leading_zeros() {
        assert_eq!(pae("", b""), b"DSSEv1 0  0 ".to_vec());
    }

    #[test]
    fn lengths_are_bytes_not_chars() {
        // "é" is 2 bytes in UTF-8; LEN counts bytes.
        assert_eq!(
            pae("é", "ü".as_bytes()),
            "DSSEv1 2 é 2 ü".as_bytes().to_vec()
        );
    }

    #[test]
    fn in_toto_payload_type_prefix() {
        let out = pae(agentworth_schema::work_receipt::IN_TOTO_PAYLOAD_TYPE, b"{}");
        assert_eq!(out, b"DSSEv1 28 application/vnd.in-toto+json 2 {}".to_vec());
    }
}

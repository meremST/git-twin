use std::fmt;

/// Git object identifier (SHA-1 or SHA-256), stored in binary form.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Oid {
    bytes: [u8; 32],
    len: u8,
}

impl Oid {
    pub fn from_hex(s: &str) -> Option<Oid> {
        let s = s.as_bytes();
        if s.len() != 40 && s.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, pair) in s.chunks(2).enumerate() {
            let hi = hex_val(pair[0])?;
            let lo = hex_val(pair[1])?;
            bytes[i] = (hi << 4) | lo;
        }
        Some(Oid {
            bytes,
            len: (s.len() / 2) as u8,
        })
    }

    pub fn from_hex_bytes(s: &[u8]) -> Option<Oid> {
        std::str::from_utf8(s).ok().and_then(Oid::from_hex)
    }

    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(self.len as usize * 2);
        for b in &self.bytes[..self.len as usize] {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
        out
    }

    pub fn short(&self, n: usize) -> String {
        let mut h = self.to_hex();
        h.truncate(n.max(4));
        h
    }

    pub fn is_zero(&self) -> bool {
        self.bytes.iter().all(|b| *b == 0)
    }

    pub fn starts_with_hex(&self, prefix: &str) -> bool {
        self.to_hex().starts_with(&prefix.to_ascii_lowercase())
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Oid({})", self.short(10))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sha1_and_sha256() {
        let h1 = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(Oid::from_hex(h1).unwrap().to_hex(), h1);
        let h2 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(Oid::from_hex(h2).unwrap().to_hex(), h2);
        assert_eq!(
            Oid::from_hex(&h1.to_uppercase()).unwrap().to_hex(),
            h1,
            "uppercase hex accepted"
        );
    }

    #[test]
    fn rejects_invalid() {
        assert!(Oid::from_hex("xyz").is_none());
        assert!(Oid::from_hex("g123456789abcdef0123456789abcdef01234567").is_none());
        assert!(Oid::from_hex("").is_none());
    }

    #[test]
    fn short_and_zero() {
        let z = Oid::from_hex(&"0".repeat(40)).unwrap();
        assert!(z.is_zero());
        assert_eq!(z.short(7), "0000000");
    }
}

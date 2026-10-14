//! Byte patterns in the pyMHF text form: `"48 8B ? ?? 05"`. `?` and `??` match any byte.

use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Pattern {
    bytes: Vec<Option<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError(pub String);

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bad pattern: {}", self.0)
    }
}

impl std::error::Error for PatternError {}

impl Pattern {
    pub fn parse(text: &str) -> Result<Self, PatternError> {
        let mut bytes = Vec::new();
        for token in text.split_whitespace() {
            if token == "?" || token == "??" {
                bytes.push(None);
            } else if token.len() == 2 {
                let b = u8::from_str_radix(token, 16).map_err(|_| PatternError(format!("token {token:?}")))?;
                bytes.push(Some(b));
            } else {
                return Err(PatternError(format!("token {token:?}")));
            }
        }
        if bytes.is_empty() {
            return Err(PatternError("empty".into()));
        }
        if bytes[0].is_none() {
            return Err(PatternError("starts with a wildcard".into()));
        }
        Ok(Self { bytes })
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The same pattern with `new` written over the tokens at `at..at + new.len()`.
    pub fn with_bytes_at(&self, at: usize, new: &[u8]) -> Result<Self, PatternError> {
        if at + new.len() > self.bytes.len() {
            return Err(PatternError(format!("patch at {at} of {} bytes runs past the pattern", new.len())));
        }
        let mut bytes = self.bytes.clone();
        for (i, b) in new.iter().enumerate() {
            bytes[at + i] = Some(*b);
        }
        Ok(Self { bytes })
    }

    pub fn matches_at(&self, hay: &[u8], pos: usize) -> bool {
        if pos + self.bytes.len() > hay.len() {
            return false;
        }
        self.bytes
            .iter()
            .zip(&hay[pos..])
            .all(|(p, h)| p.is_none_or(|b| b == *h))
    }

    /// Every offset in `hay` where the pattern matches, up to `limit` hits.
    pub fn find_all(&self, hay: &[u8], limit: usize) -> Vec<usize> {
        let mut hits = Vec::new();
        let first = self.bytes[0].expect("parse rejects a leading wildcard");
        let last_start = match hay.len().checked_sub(self.bytes.len()) {
            Some(n) => n,
            None => return hits,
        };
        let mut pos = 0;
        while pos <= last_start {
            match hay[pos..=last_start].iter().position(|&b| b == first) {
                None => break,
                Some(skip) => {
                    pos += skip;
                    if self.matches_at(hay, pos) {
                        hits.push(pos);
                        if hits.len() >= limit {
                            break;
                        }
                    }
                    pos += 1;
                }
            }
        }
        hits
    }
}

impl fmt::Debug for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .bytes
            .iter()
            .map(|b| b.map_or("?".to_string(), |b| format!("{b:02X}")))
            .collect();
        write!(f, "Pattern({})", parts.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wildcards() {
        let p = Pattern::parse("48 8B ? ?? 05").unwrap();
        assert_eq!(p.len(), 5);
        assert!(p.matches_at(&[0x48, 0x8B, 1, 2, 5], 0));
        assert!(!p.matches_at(&[0x48, 0x8B, 1, 2, 6], 0));
    }

    #[test]
    fn rejects_bad_tokens() {
        assert!(Pattern::parse("").is_err());
        assert!(Pattern::parse("? 48").is_err());
        assert!(Pattern::parse("4").is_err());
        assert!(Pattern::parse("ZZ").is_err());
    }

    #[test]
    fn finds_all_hits() {
        let p = Pattern::parse("AA ? CC").unwrap();
        let hay = [0xAA, 1, 0xCC, 0xAA, 2, 0xCC, 0xAA];
        assert_eq!(p.find_all(&hay, 10), vec![0, 3]);
        assert_eq!(p.find_all(&hay, 1), vec![0]);
        assert!(p.find_all(&[0xAA], 10).is_empty());
    }

    #[test]
    fn writes_bytes_into_pattern() {
        let p = Pattern::parse("8D 42 C6 83 F8 02 76 ?").unwrap();
        let q = p.with_bytes_at(0, &[0x8D, 0x42, 0xC5, 0x83, 0xF8, 0x01]).unwrap();
        assert!(q.matches_at(&[0x8D, 0x42, 0xC5, 0x83, 0xF8, 0x01, 0x76, 9], 0));
        assert!(p.with_bytes_at(5, &[1, 2, 3, 4]).is_err());
    }
}

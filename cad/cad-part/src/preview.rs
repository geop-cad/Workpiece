//! A small picture of a part, kept in its file: what the part looks
//! like, for telling it from the others before it is opened.

/// A small picture of a part as it is built: a PNG, with what the part
/// was when it was taken.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preview {
    /// The [stamp](crate::Part::stamp) of the part it is a picture of.
    pub of: String,
    /// The picture: a PNG file's bytes, written as base64.
    png: String,
}

/// The preview a part has, if it has one: kept in the part's file
/// beside the cache, it says nothing of the part. Two parts that differ
/// only in it are the same part — so any two of these are equal, and a
/// part does not differ from its file by having been pictured.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Pictured(pub Option<Preview>);

impl PartialEq for Pictured {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Pictured {
    /// Whether the part has no preview.
    pub fn is_none(&self) -> bool {
        self.0.is_none()
    }
}

const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl Preview {
    /// The picture `png` (a PNG file's bytes) of the part stamped `of`.
    pub fn new(of: String, png: &[u8]) -> Self {
        let mut text = String::with_capacity(png.len().div_ceil(3) * 4);
        for chunk in png.chunks(3) {
            let b = [
                chunk[0],
                *chunk.get(1).unwrap_or(&0),
                *chunk.get(2).unwrap_or(&0),
            ];
            let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            for k in 0..4 {
                if k <= chunk.len() {
                    text.push(LETTERS[(n >> (18 - 6 * k) & 63) as usize] as char);
                } else {
                    text.push('=');
                }
            }
        }
        Self { of, png: text }
    }

    /// The picture: a PNG file's bytes. Nothing, if what the file holds
    /// is no base64.
    pub fn png(&self) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(self.png.len() / 4 * 3);
        let (mut bits, mut have) = (0u32, 0);
        for c in self.png.bytes().filter(|c| *c != b'=') {
            let value = LETTERS.iter().position(|l| *l == c)?;
            bits = bits << 6 | value as u32;
            have += 6;
            if have >= 8 {
                have -= 8;
                out.push((bits >> have) as u8);
            }
        }
        Some(out)
    }
}

/// Hashes what is written to it (FNV-1a): the same on every machine and
/// in every version, unlike the standard hasher.
pub(crate) struct Stamp(pub u64);

impl Default for Stamp {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl std::io::Write for Stamp {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_keeps_its_bytes_and_makes_no_part_another() {
        for n in 0..7 {
            let bytes: Vec<u8> = (0..n)
                .map(|k: u8| k.wrapping_mul(37).wrapping_add(200))
                .collect();
            assert_eq!(Preview::new("s".into(), &bytes).png(), Some(bytes));
        }
        let pictured = Pictured(Some(Preview::new("a".into(), &[1])));
        assert_eq!(pictured, Pictured(None));
    }
}

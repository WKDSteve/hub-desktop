//! `SHA256SUMS`: the list a release or a checkout of a web variant carries, and the check of one file
//! against it.
//!
//! The hash is SHA-256 (FIPS 180-4), written here so that the hub needs no crate for the one thing it
//! hashes: a few files of an update. What a match proves is that the file is the one the list names,
//! not who made the list: it is no signature (appkit `release-kit/README.md`).

use std::path::Path;

/// The largest file the hub hashes: a setup of an app is a few dozen MiB, nothing an update fetches is
/// near this.
pub const MAX_FILE: u64 = 512 * 1024 * 1024;

#[rustfmt::skip]
const K: [u32; 64] = [
    0x428a_2f98, 0x7137_4491, 0xb5c0_fbcf, 0xe9b5_dba5, 0x3956_c25b, 0x59f1_11f1, 0x923f_82a4,
    0xab1c_5ed5, 0xd807_aa98, 0x1283_5b01, 0x2431_85be, 0x550c_7dc3, 0x72be_5d74, 0x80de_b1fe,
    0x9bdc_06a7, 0xc19b_f174, 0xe49b_69c1, 0xefbe_4786, 0x0fc1_9dc6, 0x240c_a1cc, 0x2de9_2c6f,
    0x4a74_84aa, 0x5cb0_a9dc, 0x76f9_88da, 0x983e_5152, 0xa831_c66d, 0xb003_27c8, 0xbf59_7fc7,
    0xc6e0_0bf3, 0xd5a7_9147, 0x06ca_6351, 0x1429_2967, 0x27b7_0a85, 0x2e1b_2138, 0x4d2c_6dfc,
    0x5338_0d13, 0x650a_7354, 0x766a_0abb, 0x81c2_c92e, 0x9272_2c85, 0xa2bf_e8a1, 0xa81a_664b,
    0xc24b_8b70, 0xc76c_51a3, 0xd192_e819, 0xd699_0624, 0xf40e_3585, 0x106a_a070, 0x19a4_c116,
    0x1e37_6c08, 0x2748_774c, 0x34b0_bcb5, 0x391c_0cb3, 0x4ed8_aa4a, 0x5b9c_ca4f, 0x682e_6ff3,
    0x748f_82ee, 0x78a5_636f, 0x84c8_7814, 0x8cc7_0208, 0x90be_fffa, 0xa450_6ceb, 0xbef9_a3f7,
    0xc671_78f2,
];

#[rustfmt::skip]
const H0: [u32; 8] = [
    0x6a09_e667, 0xbb67_ae85, 0x3c6e_f372, 0xa54f_f53a, 0x510e_527f, 0x9b05_688c, 0x1f83_d9ab,
    0x5be0_cd19,
];

/// SHA-256, fed in pieces.
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; 64],
    used: usize,
    length: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self {
            state: H0,
            block: [0; 64],
            used: 0,
            length: 0,
        }
    }
}

impl Sha256 {
    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);
        while !data.is_empty() {
            let take = (64 - self.used).min(data.len());
            self.block[self.used..self.used + take].copy_from_slice(&data[..take]);
            self.used += take;
            data = &data[take..];
            if self.used == 64 {
                let block = self.block;
                self.compress(&block);
                self.used = 0;
            }
        }
    }

    #[must_use]
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length.wrapping_mul(8);
        self.update(&[0x80]);
        while self.used != 56 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = [0u8; 32];
        for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            *chunk = word.to_be_bytes();
        }
        out
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (slot, chunk) in w.iter_mut().zip(block.as_chunks::<4>().0) {
            *slot = u32::from_be_bytes(*chunk);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, add) in self.state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(add);
        }
    }
}

/// The hash of `data` as 64 lowercase hex digits.
#[must_use]
pub fn hex_of(data: &[u8]) -> String {
    let mut hash = Sha256::default();
    hash.update(data);
    to_hex(&hash.finish())
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// The hash of a file, read in pieces.
///
/// # Errors
///
/// The file cannot be read, or it is larger than [`MAX_FILE`].
pub fn hex_of_file(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err("not a file of a size the hub checks".to_owned());
    }
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::default();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buf).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hash.update(&buf[..read]);
    }
    Ok(to_hex(&hash.finish()))
}

/// The hash `SHA256SUMS` names for `name`: a line `<64 hex>  <name>` (or `*<name>`, the binary mark of
/// `sha256sum`). A name listed twice with two hashes is no answer.
#[must_use]
pub fn listed(sums: &str, name: &str) -> Option<String> {
    let mut found: Option<String> = None;
    for line in sums.lines() {
        let line = line.trim();
        let Some((hash, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let file = rest.trim_start().trim_start_matches('*');
        if file != name || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let hash = hash.to_ascii_lowercase();
        match &found {
            Some(before) if *before != hash => return None,
            _ => found = Some(hash),
        }
    }
    found
}

/// How a file compares with its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Matches,
    /// The list has no line for the file (or two that disagree).
    NotListed,
    Differs,
}

/// Check the file `name` in `dir` against `SHA256SUMS` in the same folder (`sums_text` is that file).
///
/// # Errors
///
/// The file cannot be read.
pub fn check(dir: &Path, sums_text: &str, name: &str) -> Result<Check, String> {
    let Some(want) = listed(sums_text, name) else {
        return Ok(Check::NotListed);
    };
    Ok(if hex_of_file(&dir.join(name))? == want {
        Check::Matches
    } else {
        Check::Differs
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The examples of FIPS 180-2 (appendix B) and the empty input.
    #[test]
    fn the_hash_is_sha256() {
        assert_eq!(
            hex_of(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_of(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_of(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex_of(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    /// The pieces may end anywhere in a block: the hash is that of the whole.
    #[test]
    fn the_hash_does_not_depend_on_the_pieces() {
        let data: Vec<u8> = (0..300u32).map(|i| (i % 251) as u8).collect();
        let whole = hex_of(&data);
        for cut in [0, 1, 55, 56, 63, 64, 65, 128, 299] {
            let mut hash = Sha256::default();
            hash.update(&data[..cut]);
            hash.update(&data[cut..]);
            assert_eq!(to_hex(&hash.finish()), whole, "{cut}");
        }
    }

    #[test]
    fn a_line_names_its_file_exactly() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums =
            format!("{a}  tasks.html\n{b} *engine.html\n  \nnot a line\n{a}  x/tasks.html\n");
        assert_eq!(listed(&sums, "tasks.html"), Some(a.clone()));
        assert_eq!(listed(&sums, "engine.html"), Some("b".repeat(64)));
        for no in ["tasks", "TASKS.HTML", "x", ""] {
            assert_eq!(listed(&sums, no), None, "{no}");
        }
        assert_eq!(
            listed(&format!("{}  short\n", "a".repeat(63)), "short"),
            None
        );
        assert_eq!(listed(&format!("{}  z\n", "g".repeat(64)), "z"), None);
        // Two lines that disagree are no answer; the same line twice is.
        assert_eq!(
            listed(&format!("{a}  t\n{}  t\n", "c".repeat(64)), "t"),
            None
        );
        assert_eq!(listed(&format!("{a}  t\n{a}  t\n"), "t"), Some(a));
    }

    #[test]
    fn a_file_matches_differs_or_is_not_listed() {
        let dir = appkit_scratch::Scratch::new("hub-sums-check");
        std::fs::write(dir.path().join("page.html"), b"abc").unwrap();
        let good = format!("{}  page.html\n", hex_of(b"abc"));
        let bad = format!("{}  page.html\n", hex_of(b"abd"));
        assert_eq!(check(dir.path(), &good, "page.html"), Ok(Check::Matches));
        assert_eq!(check(dir.path(), &bad, "page.html"), Ok(Check::Differs));
        assert_eq!(check(dir.path(), "", "page.html"), Ok(Check::NotListed));
        assert!(check(dir.path(), &good.replace("page", "gone"), "gone.html").is_err());
    }
}

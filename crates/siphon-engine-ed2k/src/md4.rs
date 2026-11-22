use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// Standard eD2k chunk size: 9,728,000 bytes (9500 KiB).
pub const ED2K_CHUNK_SIZE: usize = 9_728_000;

/// Compute raw RFC 1320 MD4 digest (16 bytes).
pub fn md4_digest(input: &[u8]) -> [u8; 16] {
    let mut a: u32 = 0x6745_2301;
    let mut b: u32 = 0xEFCD_AB89;
    let mut c: u32 = 0x98BA_DCFE;
    let mut d: u32 = 0x1032_5476;

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(((input.len() + 9 + 63) / 64) * 64);
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0x00);
    }
    padded.extend_from_slice(&bit_len.to_le_bytes());

    for block in padded.chunks_exact(64) {
        let mut x = [0u32; 16];
        for (i, word) in block.chunks_exact(4).enumerate() {
            x[i] = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        }

        let (aa, bb, cc, dd) = (a, b, c, d);

        // Round 1
        #[inline]
        fn f(x: u32, y: u32, z: u32) -> u32 {
            (x & y) | (!x & z)
        }
        #[inline]
        fn g(x: u32, y: u32, z: u32) -> u32 {
            (x & y) | (x & z) | (y & z)
        }
        #[inline]
        fn h(x: u32, y: u32, z: u32) -> u32 {
            x ^ y ^ z
        }

        macro_rules! r1 {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr) => {
                $a = $a
                    .wrapping_add(f($b, $c, $d))
                    .wrapping_add(x[$k])
                    .rotate_left($s);
            };
        }
        macro_rules! r2 {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr) => {
                $a = $a
                    .wrapping_add(g($b, $c, $d))
                    .wrapping_add(x[$k])
                    .wrapping_add(0x5A82_7999)
                    .rotate_left($s);
            };
        }
        macro_rules! r3 {
            ($a:ident, $b:ident, $c:ident, $d:ident, $k:expr, $s:expr) => {
                $a = $a
                    .wrapping_add(h($b, $c, $d))
                    .wrapping_add(x[$k])
                    .wrapping_add(0x6ED9_EBA1)
                    .rotate_left($s);
            };
        }

        r1!(a, b, c, d, 0, 3);
        r1!(d, a, b, c, 1, 7);
        r1!(c, d, a, b, 2, 11);
        r1!(b, c, d, a, 3, 19);
        r1!(a, b, c, d, 4, 3);
        r1!(d, a, b, c, 5, 7);
        r1!(c, d, a, b, 6, 11);
        r1!(b, c, d, a, 7, 19);
        r1!(a, b, c, d, 8, 3);
        r1!(d, a, b, c, 9, 7);
        r1!(c, d, a, b, 10, 11);
        r1!(b, c, d, a, 11, 19);
        r1!(a, b, c, d, 12, 3);
        r1!(d, a, b, c, 13, 7);
        r1!(c, d, a, b, 14, 11);
        r1!(b, c, d, a, 15, 19);

        // Round 2
        r2!(a, b, c, d, 0, 3);
        r2!(d, a, b, c, 4, 5);
        r2!(c, d, a, b, 8, 9);
        r2!(b, c, d, a, 12, 13);
        r2!(a, b, c, d, 1, 3);
        r2!(d, a, b, c, 5, 5);
        r2!(c, d, a, b, 9, 9);
        r2!(b, c, d, a, 13, 13);
        r2!(a, b, c, d, 2, 3);
        r2!(d, a, b, c, 6, 5);
        r2!(c, d, a, b, 10, 9);
        r2!(b, c, d, a, 14, 13);
        r2!(a, b, c, d, 3, 3);
        r2!(d, a, b, c, 7, 5);
        r2!(c, d, a, b, 11, 9);
        r2!(b, c, d, a, 15, 13);

        // Round 3
        r3!(a, b, c, d, 0, 3);
        r3!(d, a, b, c, 8, 9);
        r3!(c, d, a, b, 4, 11);
        r3!(b, c, d, a, 12, 15);
        r3!(a, b, c, d, 2, 3);
        r3!(d, a, b, c, 10, 9);
        r3!(c, d, a, b, 6, 11);
        r3!(b, c, d, a, 14, 15);
        r3!(a, b, c, d, 1, 3);
        r3!(d, a, b, c, 9, 9);
        r3!(c, d, a, b, 5, 11);
        r3!(b, c, d, a, 13, 15);
        r3!(a, b, c, d, 3, 3);
        r3!(d, a, b, c, 11, 9);
        r3!(c, d, a, b, 7, 11);
        r3!(b, c, d, a, 15, 15);

        a = a.wrapping_add(aa);
        b = b.wrapping_add(bb);
        c = c.wrapping_add(cc);
        d = d.wrapping_add(dd);
    }

    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a.to_le_bytes());
    out[4..8].copy_from_slice(&b.to_le_bytes());
    out[8..12].copy_from_slice(&c.to_le_bytes());
    out[12..16].copy_from_slice(&d.to_le_bytes());
    out
}

/// Compute standard eD2k file hash over an in-memory buffer (uppercase 32-char hex).
pub fn ed2k_file_hash_hex(data: &[u8]) -> String {
    if data.len() < ED2K_CHUNK_SIZE {
        return hex::encode_upper(md4_digest(data));
    }
    let mut chunk_hashes = Vec::new();
    for chunk in data.chunks(ED2K_CHUNK_SIZE) {
        chunk_hashes.extend_from_slice(&md4_digest(chunk));
    }
    if data.len() % ED2K_CHUNK_SIZE == 0 {
        chunk_hashes.extend_from_slice(&md4_digest(&[]));
    }
    hex::encode_upper(md4_digest(&chunk_hashes))
}

/// Stream a file from disk in `ED2K_CHUNK_SIZE` chunks and compute its eD2k MD4 hash (uppercase hex).
pub fn ed2k_file_hash_path(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let meta_len = file.metadata()?.len();
    let mut buf = vec![0u8; ED2K_CHUNK_SIZE.min((meta_len as usize).max(1))];

    if meta_len < ED2K_CHUNK_SIZE as u64 {
        let mut all = Vec::with_capacity(meta_len as usize);
        file.read_to_end(&mut all)?;
        return Ok(hex::encode_upper(md4_digest(&all)));
    }

    let mut chunk_hashes = Vec::new();
    let mut total_read = 0u64;
    loop {
        let mut filled = 0usize;
        while filled < ED2K_CHUNK_SIZE {
            let n = file.read(&mut buf[filled..ED2K_CHUNK_SIZE])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break;
        }
        total_read += filled as u64;
        chunk_hashes.extend_from_slice(&md4_digest(&buf[..filled]));
        if filled < ED2K_CHUNK_SIZE {
            break;
        }
    }
    if total_read > 0 && total_read % (ED2K_CHUNK_SIZE as u64) == 0 {
        chunk_hashes.extend_from_slice(&md4_digest(&[]));
    }
    Ok(hex::encode_upper(md4_digest(&chunk_hashes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_rfc1320_md4_test_vectors() {
        assert_eq!(hex::encode(md4_digest(b"")), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(hex::encode(md4_digest(b"a")), "bde52cb31de33e46245e05fbdbd6fb24");
        assert_eq!(hex::encode(md4_digest(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(
            hex::encode(md4_digest(b"message digest")),
            "d9130a8164549fe818874806e1c7014b"
        );
        assert_eq!(
            hex::encode(md4_digest(b"abcdefghijklmnopqrstuvwxyz")),
            "d79e1c308aa5bbcdeea8ed63df412da9"
        );
    }

    #[test]
    fn ed2k_file_hash_matches_for_small_and_streamed_files() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("sample.bin");
        let payload = b"Siphon Downloader eD2k MD4 verification payload 1234567890";
        std::fs::write(&p, payload).unwrap();

        let mem_hash = ed2k_file_hash_hex(payload);
        let disk_hash = ed2k_file_hash_path(&p).unwrap();
        assert_eq!(mem_hash, disk_hash);
        assert_eq!(mem_hash.len(), 32);
    }
}

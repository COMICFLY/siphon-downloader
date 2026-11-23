//! Pure-Rust BitTorrent Message Stream Encryption / Protocol Encryption (MSE/PE).
//!
//! Implements 768-bit Diffie-Hellman key exchange + SHA-1 key derivation + RC4-drop1024
//! full-stream encryption for both outgoing (initiator) and incoming (responder) BitTorrent
//! peer connections, enabling compatibility with encryption-enforcing peers and bypassing
//! plaintext BT protocol sniffers (such as TUN proxies or ISP throttlers).

use std::{
    io::IoSliceMut,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use anyhow::{Context as _, bail};
use librqbit_core::hash_id::Id20;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha1w::{ISha1, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf as TokioReadBuf};

use crate::{
    type_aliases::{BoxAsyncReadVectored, BoxAsyncWrite},
    vectored_traits::AsyncReadVectored,
};

/// MSE/PE encryption policy for BitTorrent peer connections.
#[derive(Debug, Default, Copy, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionMode {
    /// Use plaintext BitTorrent handshakes only (upstream librqbit default).
    #[default]
    Disabled,
    /// Attempt MSE/PE RC4 encrypted handshake first; fall back to plaintext if peer rejects MSE.
    PreferEncrypted,
    /// Require MSE/PE RC4 encryption for all outgoing and incoming peer connections.
    ForceEncrypted,
}

const CRYPTO_PLAINTEXT: u32 = 0x0000_0001;
const CRYPTO_RC4: u32 = 0x0000_0002;
const BT_PROTOCOL_HEADER: &[u8; 20] = b"\x13BitTorrent protocol";
const MAX_PAD_LEN: usize = 512;

/// Official MSE/PE 768-bit Diffie-Hellman prime P (big-endian, 96 bytes).
const DH_P_BE: [u8; 96] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC9, 0x0F, 0xDA, 0xA2, 0x21, 0x68, 0xC2, 0x34,
    0xC4, 0xC6, 0x62, 0x8B, 0x80, 0xDC, 0x1C, 0xD1, 0x29, 0x02, 0x4E, 0x08, 0x8A, 0x67, 0xCC, 0x74,
    0x02, 0x0B, 0xBE, 0xA6, 0x3B, 0x13, 0x9B, 0x22, 0x51, 0x4A, 0x08, 0x79, 0x8E, 0x34, 0x04, 0xDD,
    0xEF, 0x95, 0x19, 0xB3, 0xCD, 0x3A, 0x43, 0x1B, 0x30, 0x2B, 0x0A, 0x6D, 0xF2, 0x5F, 0x14, 0x37,
    0x4F, 0xE1, 0x35, 0x6D, 0x6D, 0x51, 0xC2, 0x45, 0xE4, 0x85, 0xB5, 0x76, 0x62, 0x5E, 0x7E, 0xC6,
    0xF4, 0x4C, 0x42, 0xE9, 0xA6, 0x3A, 0x36, 0x21, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x05, 0x63,
];

/// Fixed-width 768-bit unsigned integer stored as 12 little-endian `u64` limbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct U768([u64; 12]);

impl U768 {
    const ONE: Self = Self([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    const TWO: Self = Self([2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    fn dh_p() -> Self {
        Self::from_be_bytes(&DH_P_BE)
    }

    fn from_be_bytes(bytes: &[u8; 96]) -> Self {
        let mut limbs = [0u64; 12];
        for i in 0..12 {
            let offset = (11 - i) * 8;
            limbs[i] = u64::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]);
        }
        Self(limbs)
    }

    fn to_be_bytes(self) -> [u8; 96] {
        let mut out = [0u8; 96];
        for i in 0..12 {
            let offset = (11 - i) * 8;
            out[offset..offset + 8].copy_from_slice(&self.0[i].to_be_bytes());
        }
        out
    }

    fn ge_12(a: &[u64; 12], b: &[u64; 12]) -> bool {
        for i in (0..12).rev() {
            if a[i] != b[i] {
                return a[i] > b[i];
            }
        }
        true
    }

    fn sub_in_place_13(rem: &mut [u64; 13], m: &[u64; 12]) {
        let mut borrow = 0u64;
        for i in 0..12 {
            let (d1, b1) = rem[i].overflowing_sub(m[i]);
            let (d2, b2) = d1.overflowing_sub(borrow);
            rem[i] = d2;
            borrow = u64::from(b1) + u64::from(b2);
        }
        rem[12] = rem[12].wrapping_sub(borrow);
    }

    fn reduce_once(mut self, p: &Self) -> Self {
        if Self::ge_12(&self.0, &p.0) {
            let mut borrow = 0u64;
            for i in 0..12 {
                let (d1, b1) = self.0[i].overflowing_sub(p.0[i]);
                let (d2, b2) = d1.overflowing_sub(borrow);
                self.0[i] = d2;
                borrow = u64::from(b1) + u64::from(b2);
            }
        }
        self
    }

    /// Compute `(a * b) mod p` where `p` is the 768-bit prime modulus.
    fn mul_mod(a: Self, b: Self, p: &Self) -> Self {
        let a = a.reduce_once(p);
        let b = b.reduce_once(p);

        // Full 1536-bit schoolbook product
        let mut prod = [0u64; 24];
        for i in 0..12 {
            let ai = u128::from(a.0[i]);
            let mut carry = 0u128;
            for j in 0..12 {
                let cur = ai * u128::from(b.0[j]) + u128::from(prod[i + j]) + carry;
                prod[i + j] = cur as u64;
                carry = cur >> 64;
            }
            prod[i + 12] = carry as u64;
        }

        // Since a < p and b < p < 2^768, floor(a*b / 2^768) < p strictly holds.
        let mut rem = [0u64; 13];
        rem[..12].copy_from_slice(&prod[12..24]);

        for limb_idx in (0..12).rev() {
            let limb = prod[limb_idx];
            for bit_idx in (0..64).rev() {
                let mut carry = (limb >> bit_idx) & 1;
                for r in &mut rem {
                    let next_carry = *r >> 63;
                    *r = (*r << 1) | carry;
                    carry = next_carry;
                }
                if rem[12] > 0 || Self::ge_12(rem[..12].try_into().unwrap(), &p.0) {
                    Self::sub_in_place_13(&mut rem, &p.0);
                }
            }
        }

        let mut out = [0u64; 12];
        out.copy_from_slice(&rem[..12]);
        Self(out)
    }

    /// Compute `base^exp_be mod P` where `exp_be` is a big-endian byte slice.
    fn mod_pow_p(base: Self, exp_be: &[u8]) -> Self {
        let p = Self::dh_p();
        let base = base.reduce_once(&p);
        let mut acc = Self::ONE;
        for &byte in exp_be {
            for bit_idx in (0..8).rev() {
                acc = Self::mul_mod(acc, acc, &p);
                if ((byte >> bit_idx) & 1) != 0 {
                    acc = Self::mul_mod(acc, base, &p);
                }
            }
        }
        acc
    }
}

/// RC4 stream cipher (with MSE/PE 1024-byte initial keystream discard).
#[derive(Clone)]
pub(crate) struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    pub(crate) fn new_drop1024(key: &[u8]) -> Self {
        let mut s = [0u8; 256];
        for (idx, slot) in s.iter_mut().enumerate() {
            *slot = idx as u8;
        }
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, usize::from(j));
        }
        let mut cipher = Self { s, i: 0, j: 0 };
        let mut discard = [0u8; 1024];
        cipher.apply(&mut discard);
        cipher
    }

    pub(crate) fn apply(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[usize::from(self.i)]);
            self.s.swap(usize::from(self.i), usize::from(self.j));
            let idx = self.s[usize::from(self.i)].wrapping_add(self.s[usize::from(self.j)]);
            *b ^= self.s[usize::from(idx)];
        }
    }
}

fn sha1_parts(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finish()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Reader that replays a buffered prefix before delegating directly to the underlying stream.
pub(crate) struct PrefixedReader {
    inner: BoxAsyncReadVectored,
    prefix: Vec<u8>,
    pos: usize,
}

impl PrefixedReader {
    pub(crate) fn new(inner: BoxAsyncReadVectored, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix,
            pos: 0,
        }
    }
}

impl AsyncRead for PrefixedReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut TokioReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.pos < this.prefix.len() {
            let rem = &this.prefix[this.pos..];
            let n = rem.len().min(buf.remaining());
            buf.put_slice(&rem[..n]);
            this.pos += n;
            if this.pos == this.prefix.len() {
                this.prefix = Vec::new();
                this.pos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl AsyncReadVectored for PrefixedReader {
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.pos < this.prefix.len() {
            let mut copied = 0usize;
            for slice in vec.iter_mut() {
                let rem = &this.prefix[this.pos..];
                if rem.is_empty() {
                    break;
                }
                let n = rem.len().min(slice.len());
                slice[..n].copy_from_slice(&rem[..n]);
                this.pos += n;
                copied += n;
            }
            if this.pos == this.prefix.len() {
                this.prefix = Vec::new();
                this.pos = 0;
            }
            return Poll::Ready(Ok(copied));
        }
        Pin::new(&mut this.inner).poll_read_vectored(cx, vec)
    }
}

/// RC4-decrypting reader with optional already-decrypted prefix bytes.
pub(crate) struct Rc4Reader {
    inner: BoxAsyncReadVectored,
    rc4: Rc4,
    plain_prefix: Vec<u8>,
    pos: usize,
}

impl Rc4Reader {
    pub(crate) fn new(inner: BoxAsyncReadVectored, rc4: Rc4, plain_prefix: Vec<u8>) -> Self {
        Self {
            inner,
            rc4,
            plain_prefix,
            pos: 0,
        }
    }
}

impl AsyncRead for Rc4Reader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut TokioReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.pos < this.plain_prefix.len() {
            let rem = &this.plain_prefix[this.pos..];
            let n = rem.len().min(buf.remaining());
            buf.put_slice(&rem[..n]);
            this.pos += n;
            if this.pos == this.plain_prefix.len() {
                this.plain_prefix = Vec::new();
                this.pos = 0;
            }
            return Poll::Ready(Ok(()));
        }
        let before = buf.filled().len();
        std::task::ready!(Pin::new(&mut this.inner).poll_read(cx, buf))?;
        let filled = buf.filled_mut();
        if filled.len() > before {
            this.rc4.apply(&mut filled[before..]);
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncReadVectored for Rc4Reader {
    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        vec: &mut [IoSliceMut<'_>],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if this.pos < this.plain_prefix.len() {
            let mut copied = 0usize;
            for slice in vec.iter_mut() {
                let rem = &this.plain_prefix[this.pos..];
                if rem.is_empty() {
                    break;
                }
                let n = rem.len().min(slice.len());
                slice[..n].copy_from_slice(&rem[..n]);
                this.pos += n;
                copied += n;
            }
            if this.pos == this.plain_prefix.len() {
                this.plain_prefix = Vec::new();
                this.pos = 0;
            }
            return Poll::Ready(Ok(copied));
        }
        let n = std::task::ready!(Pin::new(&mut this.inner).poll_read_vectored(cx, vec))?;
        let mut remaining = n;
        for slice in vec.iter_mut() {
            if remaining == 0 {
                break;
            }
            let take = remaining.min(slice.len());
            this.rc4.apply(&mut slice[..take]);
            remaining -= take;
        }
        Poll::Ready(Ok(n))
    }
}

/// RC4-encrypting writer that guarantees keystream consistency across partial writes and `Poll::Pending`.
pub(crate) struct Rc4Writer {
    inner: BoxAsyncWrite,
    rc4: Rc4,
    pending: Vec<u8>,
    written: usize,
    orig_len: usize,
}

impl Rc4Writer {
    pub(crate) fn new(inner: BoxAsyncWrite, rc4: Rc4) -> Self {
        Self {
            inner,
            rc4,
            pending: Vec::new(),
            written: 0,
            orig_len: 0,
        }
    }

    fn poll_drain_pending(&mut self, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        while self.written < self.pending.len() {
            let n = std::task::ready!(
                Pin::new(&mut self.inner).poll_write(cx, &self.pending[self.written..])
            )?;
            if n == 0 {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "underlying stream returned 0 bytes written in Rc4Writer",
                )));
            }
            self.written += n;
        }
        self.pending.clear();
        self.written = 0;
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for Rc4Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        if buf.is_empty() && this.pending.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if this.pending.is_empty() {
            let take = buf.len().min(32 * 1024);
            this.pending.extend_from_slice(&buf[..take]);
            this.rc4.apply(&mut this.pending);
            this.written = 0;
            this.orig_len = take;
        }
        std::task::ready!(this.poll_drain_pending(cx))?;
        let done = this.orig_len;
        this.orig_len = 0;
        Poll::Ready(Ok(done))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.poll_drain_pending(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        std::task::ready!(this.poll_drain_pending(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

async fn read_at_least(
    reader: &mut BoxAsyncReadVectored,
    buf: &mut Vec<u8>,
    target_len: usize,
    timeout: Duration,
) -> anyhow::Result<()> {
    let mut tmp = [0u8; 512];
    let deadline = tokio::time::Instant::now() + timeout;
    while buf.len() < target_len {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            bail!("MSE handshake timed out while reading");
        }
        let n = tokio::time::timeout(rem, reader.read(&mut tmp))
            .await
            .context("MSE handshake read timeout")?
            .context("MSE handshake read error")?;
        if n == 0 {
            bail!("peer closed connection during MSE handshake");
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    Ok(())
}

/// Perform an outgoing (initiator) MSE/PE RC4 handshake, piggybacking `initial_bt_handshake`
/// inside `IA` so no extra round-trip is wasted.
///
/// On success, the returned `(reader, writer)` are RC4-encrypted streams ready to immediately
/// read the peer's BitTorrent handshake (our BitTorrent handshake has already been sent in `IA`).
pub(crate) async fn handshake_outgoing(
    mut read: BoxAsyncReadVectored,
    mut write: BoxAsyncWrite,
    info_hash: Id20,
    initial_bt_handshake: &[u8],
    rw_timeout: Duration,
) -> anyhow::Result<(BoxAsyncReadVectored, BoxAsyncWrite)> {
    // Step 1: Send Y_a (96 bytes) || PadA (0..=48 random bytes)
    let mut xa = [0u8; 20];
    let pad_a_len: usize;
    let mut pad_a = [0u8; 48];
    let pad_c_len: usize;
    {
        let mut rng = rand::rng();
        rng.fill(&mut xa);
        pad_a_len = rng.random_range(0..=48usize);
        rng.fill(&mut pad_a[..pad_a_len]);
        pad_c_len = rng.random_range(0..=32usize);
    }

    let ya = U768::mod_pow_p(U768::TWO, &xa).to_be_bytes();
    let mut msg1 = Vec::with_capacity(96 + pad_a_len);
    msg1.extend_from_slice(&ya);
    msg1.extend_from_slice(&pad_a[..pad_a_len]);

    tokio::time::timeout(rw_timeout, write.write_all(&msg1))
        .await
        .context("MSE write Y_a timeout")?
        .context("MSE write Y_a failed")?;

    // Step 2: Receive Y_b (first 96 bytes) and compute shared secret S = Y_b^X_a mod P
    let mut rx_buf = Vec::with_capacity(512);
    read_at_least(&mut read, &mut rx_buf, 96, rw_timeout).await?;
    let yb_bytes: [u8; 96] = rx_buf[..96].try_into().unwrap();
    let mut tail = rx_buf.split_off(96);

    let shared_s = U768::mod_pow_p(U768::from_be_bytes(&yb_bytes), &xa).to_be_bytes();

    // Step 3: Derive hashes & RC4 states, send req1 || (req2 ^ req3) || ENCRYPT(VC || provide || PadC || IA)
    let req1 = sha1_parts(&[b"req1", &shared_s]);
    let req2 = sha1_parts(&[b"req2", &info_hash.0]);
    let req3 = sha1_parts(&[b"req3", &shared_s]);
    let mut req2_xor_req3 = [0u8; 20];
    for idx in 0..20 {
        req2_xor_req3[idx] = req2[idx] ^ req3[idx];
    }

    let key_a = sha1_parts(&[b"keyA", &shared_s, &info_hash.0]);
    let key_b = sha1_parts(&[b"keyB", &shared_s, &info_hash.0]);
    let mut enc_rc4 = Rc4::new_drop1024(&key_a);
    let mut dec_rc4 = Rc4::new_drop1024(&key_b);

    let ia_len = u16::try_from(initial_bt_handshake.len())
        .context("initial BT handshake exceeds u16 length")?;
    let pad_c_len_u16 = u16::try_from(pad_c_len).context("PadC exceeds u16 length")?;

    let mut encrypted_part = Vec::with_capacity(8 + 4 + 2 + pad_c_len + 2 + initial_bt_handshake.len());
    encrypted_part.extend_from_slice(&[0u8; 8]); // VC
    encrypted_part.extend_from_slice(&CRYPTO_RC4.to_be_bytes()); // request full RC4 stream encryption
    encrypted_part.extend_from_slice(&pad_c_len_u16.to_be_bytes());
    encrypted_part.resize(encrypted_part.len() + pad_c_len, 0u8);
    encrypted_part.extend_from_slice(&ia_len.to_be_bytes());
    encrypted_part.extend_from_slice(initial_bt_handshake);
    enc_rc4.apply(&mut encrypted_part);

    let mut msg3 = Vec::with_capacity(40 + encrypted_part.len());
    msg3.extend_from_slice(&req1);
    msg3.extend_from_slice(&req2_xor_req3);
    msg3.extend_from_slice(&encrypted_part);

    tokio::time::timeout(rw_timeout, write.write_all(&msg3))
        .await
        .context("MSE write req/IA timeout")?
        .context("MSE write req/IA failed")?;

    // Step 4: Locate ENCRYPT_B(VC) after optional PadB (0..=512 bytes)
    let mut expected_vc_enc = [0u8; 8];
    dec_rc4.clone().apply(&mut expected_vc_enc);

    let max_scan = MAX_PAD_LEN + 8;
    let deadline = tokio::time::Instant::now() + rw_timeout;
    let vc_pos = loop {
        if let Some(pos) = find_subslice(&tail, &expected_vc_enc) {
            if pos > MAX_PAD_LEN {
                bail!("MSE PadB exceeded 512 bytes");
            }
            break pos;
        }
        if tail.len() >= max_scan {
            bail!("MSE verification constant (VC) not found within PadB window");
        }
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            bail!("MSE timed out waiting for peer VC");
        }
        let mut tmp = [0u8; 256];
        let n = tokio::time::timeout(rem, read.read(&mut tmp))
            .await
            .context("MSE read VC timeout")?
            .context("MSE read VC failed")?;
        if n == 0 {
            bail!("peer disconnected while waiting for MSE VC");
        }
        tail.extend_from_slice(&tmp[..n]);
    };

    // Drop PadB and ensure VC(8) + crypto_select(4) + len(PadD)(2) = 14 bytes
    let mut rest = tail.split_off(vc_pos);
    let rem_timeout = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .max(Duration::from_millis(500));
    read_at_least(&mut read, &mut rest, 14, rem_timeout).await?;

    let mut hdr = [0u8; 14];
    hdr.copy_from_slice(&rest[..14]);
    dec_rc4.apply(&mut hdr);
    if hdr[..8] != [0u8; 8] {
        bail!("MSE VC verification failed");
    }
    let crypto_select = u32::from_be_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    let pad_d_len = usize::from(u16::from_be_bytes([hdr[12], hdr[13]]));
    if pad_d_len > MAX_PAD_LEN {
        bail!("MSE PadD length {pad_d_len} exceeds 512 bytes");
    }
    if crypto_select != CRYPTO_RC4 {
        bail!("peer selected unsupported MSE crypto method 0x{crypto_select:08x}");
    }

    let need = 14 + pad_d_len;
    read_at_least(&mut read, &mut rest, need, rem_timeout).await?;
    // Advance decryptor across PadD
    dec_rc4.apply(&mut rest[14..need]);

    // Decrypt any trailing bytes already read (e.g. peer's BitTorrent handshake!)
    let mut leftover = rest.split_off(need);
    if !leftover.is_empty() {
        dec_rc4.apply(&mut leftover);
    }

    Ok((
        Box::new(Rc4Reader::new(read, dec_rc4, leftover)),
        Box::new(Rc4Writer::new(write, enc_rc4)),
    ))
}

/// Inspect an incoming peer connection: transparently handles both plaintext BT handshakes
/// and MSE/PE (RC4 or plaintext-downgraded) handshakes according to `mode`.
///
/// Returns `(reader, writer, used_rc4)` ready for `ReadBuf::read_handshake`.
pub(crate) async fn handshake_incoming(
    mut read: BoxAsyncReadVectored,
    mut write: BoxAsyncWrite,
    known_hashes: &[Id20],
    mode: EncryptionMode,
    rw_timeout: Duration,
) -> anyhow::Result<(BoxAsyncReadVectored, BoxAsyncWrite, bool)> {
    let mut buf = Vec::with_capacity(256);
    read_at_least(&mut read, &mut buf, 20, rw_timeout).await?;

    // Fast path: standard plaintext BitTorrent header `\x13BitTorrent protocol`
    if &buf[..20] == BT_PROTOCOL_HEADER {
        if mode == EncryptionMode::ForceEncrypted {
            bail!("incoming plaintext BitTorrent connection rejected (ForceEncrypted)");
        }
        return Ok((Box::new(PrefixedReader::new(read, buf)), write, false));
    }

    if mode == EncryptionMode::Disabled {
        bail!("incoming non-plaintext / MSE connection rejected (encryption disabled)");
    }

    // MSE/PE responder path: finish reading Y_a (96 bytes total)
    read_at_least(&mut read, &mut buf, 96, rw_timeout).await?;
    let ya_bytes: [u8; 96] = buf[..96].try_into().unwrap();
    let mut tail = buf.split_off(96);

    let mut xb = [0u8; 20];
    let pad_b_len: usize;
    let mut pad_b = [0u8; 32];
    let pad_d_len: usize;
    {
        let mut rng = rand::rng();
        rng.fill(&mut xb);
        pad_b_len = rng.random_range(0..=32usize);
        rng.fill(&mut pad_b[..pad_b_len]);
        pad_d_len = rng.random_range(0..=32usize);
    }

    let yb = U768::mod_pow_p(U768::TWO, &xb).to_be_bytes();
    let shared_s = U768::mod_pow_p(U768::from_be_bytes(&ya_bytes), &xb).to_be_bytes();

    // Send Y_b || PadB
    let mut msg2 = Vec::with_capacity(96 + pad_b_len);
    msg2.extend_from_slice(&yb);
    msg2.extend_from_slice(&pad_b[..pad_b_len]);
    tokio::time::timeout(rw_timeout, write.write_all(&msg2))
        .await
        .context("MSE responder write Y_b timeout")?
        .context("MSE responder write Y_b failed")?;

    // Scan for HASH('req1', S) after optional PadA (0..=512 bytes)
    let req1 = sha1_parts(&[b"req1", &shared_s]);
    let deadline = tokio::time::Instant::now() + rw_timeout;
    let max_scan = MAX_PAD_LEN + 20;
    let req1_pos = loop {
        if let Some(pos) = find_subslice(&tail, &req1) {
            if pos > MAX_PAD_LEN {
                bail!("MSE PadA exceeded 512 bytes");
            }
            break pos;
        }
        if tail.len() >= max_scan {
            bail!("MSE req1 marker not found within PadA window");
        }
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            bail!("MSE responder timed out waiting for req1");
        }
        let mut tmp = [0u8; 256];
        let n = tokio::time::timeout(rem, read.read(&mut tmp))
            .await
            .context("MSE responder read req1 timeout")?
            .context("MSE responder read req1 failed")?;
        if n == 0 {
            bail!("peer disconnected before MSE req1");
        }
        tail.extend_from_slice(&tmp[..n]);
    };

    let mut rest = tail.split_off(req1_pos);
    let rem_timeout = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .max(Duration::from_millis(500));
    // Need req1(20) + req2_xor_req3(20) + enc_hdr(14) = 54 bytes
    read_at_least(&mut read, &mut rest, 54, rem_timeout).await?;

    let req3 = sha1_parts(&[b"req3", &shared_s]);
    let mut req2 = [0u8; 20];
    for idx in 0..20 {
        req2[idx] = rest[20 + idx] ^ req3[idx];
    }

    let matched_hash = known_hashes
        .iter()
        .copied()
        .find(|ih| sha1_parts(&[b"req2", &ih.0]) == req2)
        .context("incoming MSE connection did not match any active torrent info_hash")?;

    let key_a = sha1_parts(&[b"keyA", &shared_s, &matched_hash.0]);
    let key_b = sha1_parts(&[b"keyB", &shared_s, &matched_hash.0]);
    let mut dec_rc4 = Rc4::new_drop1024(&key_a);
    let mut enc_rc4 = Rc4::new_drop1024(&key_b);

    let mut hdr = [0u8; 14];
    hdr.copy_from_slice(&rest[40..54]);
    dec_rc4.apply(&mut hdr);
    if hdr[..8] != [0u8; 8] {
        bail!("MSE responder VC verification failed");
    }
    let crypto_provide = u32::from_be_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]);
    let pad_c_len = usize::from(u16::from_be_bytes([hdr[12], hdr[13]]));
    if pad_c_len > MAX_PAD_LEN {
        bail!("MSE PadC length {pad_c_len} exceeds 512 bytes");
    }

    let ia_len_offset = 54 + pad_c_len;
    read_at_least(&mut read, &mut rest, ia_len_offset + 2, rem_timeout).await?;
    dec_rc4.apply(&mut rest[54..ia_len_offset + 2]);
    let ia_len = usize::from(u16::from_be_bytes([
        rest[ia_len_offset],
        rest[ia_len_offset + 1],
    ]));
    if ia_len > 4096 {
        bail!("MSE IA length {ia_len} exceeds limit");
    }

    let after_ia = ia_len_offset + 2 + ia_len;
    read_at_least(&mut read, &mut rest, after_ia, rem_timeout).await?;
    let mut ia = rest[ia_len_offset + 2..after_ia].to_vec();
    if !ia.is_empty() {
        dec_rc4.apply(&mut ia);
    }

    let crypto_select = if (crypto_provide & CRYPTO_RC4) != 0 {
        CRYPTO_RC4
    } else if (crypto_provide & CRYPTO_PLAINTEXT) != 0 && mode != EncryptionMode::ForceEncrypted {
        CRYPTO_PLAINTEXT
    } else {
        bail!("MSE peer offered incompatible crypto_provide 0x{crypto_provide:08x}");
    };

    let pad_d_len_u16 = u16::try_from(pad_d_len).context("PadD exceeds u16 length")?;
    let mut msg4 = Vec::with_capacity(14 + pad_d_len);
    msg4.extend_from_slice(&[0u8; 8]); // VC
    msg4.extend_from_slice(&crypto_select.to_be_bytes());
    msg4.extend_from_slice(&pad_d_len_u16.to_be_bytes());
    msg4.resize(msg4.len() + pad_d_len, 0u8);
    enc_rc4.apply(&mut msg4);

    tokio::time::timeout(rem_timeout, write.write_all(&msg4))
        .await
        .context("MSE responder write msg4 timeout")?
        .context("MSE responder write msg4 failed")?;

    let mut leftover = rest.split_off(after_ia);
    if crypto_select == CRYPTO_RC4 {
        if !leftover.is_empty() {
            dec_rc4.apply(&mut leftover);
        }
        ia.extend_from_slice(&leftover);
        Ok((
            Box::new(Rc4Reader::new(read, dec_rc4, ia)),
            Box::new(Rc4Writer::new(write, enc_rc4)),
            true,
        ))
    } else {
        ia.extend_from_slice(&leftover);
        Ok((Box::new(PrefixedReader::new(read, ia)), write, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vectored_traits::AsyncReadVectoredIntoCompat;

    fn decode_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn dh_768_matches_reference_vectors() {
        let xa = decode_hex("0123456789abcdef0123456789abcdef01234567");
        let xb = decode_hex("fedcba9876543210fedcba9876543210fedcba98");

        let ya = U768::mod_pow_p(U768::TWO, &xa).to_be_bytes();
        let yb = U768::mod_pow_p(U768::TWO, &xb).to_be_bytes();

        let expected_ya = decode_hex(
            "6fd4bc7aa649593205ec30348a3ccc737b61fa01e9e1762c2c53eb69033afecb\
             df7c13b8ac3643af78d0760b0f42db009f2b96c970f009d060faf617f117d0f1\
             c221cea0561b9a86e852fc70a6f09ad0f82378603aa5e56b811deb3f534bf276",
        );
        let expected_yb = decode_hex(
            "e2666b9cffca63998c8e806db50c47ac12512b327c5ec48d62e0a43c2961afcf\
             136e7219fe2ec51e656b2f01ed089dd8cc53c78e7d650df59a752131bcf8ffff\
             122144bf10c6269b47886d6af58389223c66153ebb5e3dc18ea849ceb60ac18e",
        );
        let expected_s = decode_hex(
            "0bf59f468e43b9f22a5fd0ccbc9bd7825a7495f6e670c31533db364b536ee297\
             a737d565ff7829d4c397de16aec19f893e3d28eaa14eaf7e17e1026697089fdf\
             f81c7dc7e4c65e6211443d9dd3e5d8b0c0e8b2888af39637af18a75cc1b17685",
        );

        assert_eq!(&ya[..], &expected_ya[..]);
        assert_eq!(&yb[..], &expected_yb[..]);

        let s1 = U768::mod_pow_p(U768::from_be_bytes(&yb), &xa).to_be_bytes();
        let s2 = U768::mod_pow_p(U768::from_be_bytes(&ya), &xb).to_be_bytes();
        assert_eq!(&s1[..], &expected_s[..]);
        assert_eq!(&s2[..], &expected_s[..]);
    }

    #[tokio::test]
    async fn mse_outgoing_and_incoming_roundtrip_stream_encryption() {
        let info_hash = Id20::new([0x42u8; 20]);
        let mut client_hs = Vec::with_capacity(68);
        client_hs.extend_from_slice(BT_PROTOCOL_HEADER);
        client_hs.extend_from_slice(&[0u8; 8]);
        client_hs.extend_from_slice(&info_hash.0);
        client_hs.extend_from_slice(b"-rQ9100-clientpeerid");

        let mut server_hs = Vec::with_capacity(68);
        server_hs.extend_from_slice(BT_PROTOCOL_HEADER);
        server_hs.extend_from_slice(&[0u8; 8]);
        server_hs.extend_from_slice(&info_hash.0);
        server_hs.extend_from_slice(b"-rQ9100-serverpeerid");

        let (a, b) = tokio::io::duplex(4096);
        let (a_r, a_w) = tokio::io::split(a);
        let (b_r, b_w) = tokio::io::split(b);

        let client_hs_clone = client_hs.clone();
        let client_task = tokio::spawn(async move {
            handshake_outgoing(
                Box::new(a_r.into_vectored_compat()),
                Box::new(a_w),
                info_hash,
                &client_hs_clone,
                Duration::from_secs(5),
            )
            .await
            .unwrap()
        });

        let server_task = tokio::spawn(async move {
            handshake_incoming(
                Box::new(b_r.into_vectored_compat()),
                Box::new(b_w),
                &[info_hash],
                EncryptionMode::PreferEncrypted,
                Duration::from_secs(5),
            )
            .await
            .unwrap()
        });

        let (mut cr, mut cw) = client_task.await.unwrap();
        let (mut sr, mut sw, used_rc4) = server_task.await.unwrap();
        assert!(used_rc4);

        // Server receives client's piggybacked BT handshake transparently
        let mut got_client_hs = [0u8; 68];
        sr.read_exact(&mut got_client_hs).await.unwrap();
        assert_eq!(&got_client_hs[..], &client_hs[..]);

        // Server replies with its BT handshake + a large 128KB payload to test RC4 streaming
        let payload: Vec<u8> = (0..131_072).map(|i| (i * 31 % 251) as u8).collect();
        let payload_send = payload.clone();
        let write_fut = tokio::spawn(async move {
            sw.write_all(&server_hs).await.unwrap();
            sw.write_all(&payload_send).await.unwrap();
            sw.flush().await.unwrap();
        });

        let mut got_server_hs = [0u8; 68];
        cr.read_exact(&mut got_server_hs).await.unwrap();
        assert_eq!(&got_server_hs[..20], BT_PROTOCOL_HEADER);
        assert_eq!(&got_server_hs[48..68], b"-rQ9100-serverpeerid");

        let mut got_payload = vec![0u8; payload.len()];
        cr.read_exact(&mut got_payload).await.unwrap();
        assert_eq!(got_payload, payload);

        // Client writes back through Rc4Writer
        cw.write_all(b"encrypted-piece-request-ok").await.unwrap();
        let mut got_back = [0u8; 26];
        sr.read_exact(&mut got_back).await.unwrap();
        assert_eq!(&got_back, b"encrypted-piece-request-ok");

        write_fut.await.unwrap();
    }
}

//! Thin wrappers around the RustCrypto primitives KDBX uses. No algorithm is
//! implemented here; this only wires keys, IVs and parameters together.

use aes::Aes256;
use cipher::block_padding::NoPadding;
use cipher::{
    BlockCipherEncrypt, BlockModeDecrypt, BlockModeEncrypt, KeyInit, KeyIvInit, StreamCipher,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha512};
use zeroize::Zeroizing;

use crate::error::{Error, Result};

pub type Key32 = Zeroizing<[u8; 32]>;

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn sha512(parts: &[&[u8]]) -> Zeroizing<[u8; 64]> {
    let mut h = Sha512::new();
    for p in parts {
        h.update(p);
    }
    Zeroizing::new(h.finalize().into())
}

fn hmac_of(key: &[u8], parts: &[&[u8]]) -> Hmac<Sha256> {
    // HMAC accepts keys of any length
    let mut m = <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m
}

pub fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    hmac_of(key, parts).finalize().into_bytes().into()
}

/// Constant-time comparison of an HMAC tag.
pub fn hmac_verify(key: &[u8], parts: &[&[u8]], tag: &[u8]) -> bool {
    hmac_of(key, parts).verify_slice(tag).is_ok()
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    // The OS random source failing is not something a caller can handle;
    // carrying on with zeros would be far worse.
    getrandom::fill(&mut b).expect("the operating system's random number generator failed");
    b
}

pub fn random_vec(len: usize) -> Vec<u8> {
    let mut b = vec![0u8; len];
    getrandom::fill(&mut b).expect("the operating system's random number generator failed");
    b
}

// ---------------------------------------------------------------- outer cipher

pub const CIPHER_AES256: [u8; 16] = [
    0x31, 0xc1, 0xf2, 0xe6, 0xbf, 0x71, 0x43, 0x50, 0xbe, 0x58, 0x05, 0x21, 0x6a, 0xfc, 0x5a, 0xff,
];
pub const CIPHER_TWOFISH: [u8; 16] = [
    0xad, 0x68, 0xf2, 0x9f, 0x57, 0x6f, 0x4b, 0xb9, 0xa3, 0x6a, 0xd4, 0x7a, 0xf9, 0x65, 0x34, 0x6c,
];
pub const CIPHER_CHACHA20: [u8; 16] = [
    0xd6, 0x03, 0x8a, 0x2b, 0x8b, 0x6f, 0x4c, 0xb5, 0xa5, 0x24, 0x33, 0x9a, 0x31, 0xdb, 0xb5, 0x9a,
];

/// The cipher that encrypts the whole payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cipher {
    Aes256,
    Twofish,
    ChaCha20,
}

impl Cipher {
    pub fn from_uuid(id: &[u8]) -> Result<Cipher> {
        match id {
            x if x == CIPHER_AES256 => Ok(Cipher::Aes256),
            x if x == CIPHER_TWOFISH => Ok(Cipher::Twofish),
            x if x == CIPHER_CHACHA20 => Ok(Cipher::ChaCha20),
            _ => Err(Error::Unsupported("encryption algorithm")),
        }
    }

    pub fn uuid(self) -> [u8; 16] {
        match self {
            Cipher::Aes256 => CIPHER_AES256,
            Cipher::Twofish => CIPHER_TWOFISH,
            Cipher::ChaCha20 => CIPHER_CHACHA20,
        }
    }

    pub fn iv_len(self) -> usize {
        match self {
            Cipher::Aes256 | Cipher::Twofish => 16,
            Cipher::ChaCha20 => 12,
        }
    }

    /// Decrypts without removing the padding: KDBX 3.1 checks its start
    /// bytes first, so that a wrong key is not mistaken for a damaged file.
    pub fn decrypt_raw(self, key: &[u8; 32], iv: &[u8], data: &mut [u8]) -> Result<()> {
        let bad_iv = Error::Corrupt("bad encryption IV");
        let bad_len = Error::Corrupt("encrypted data has a bad length");
        match self {
            Cipher::Aes256 => {
                cbc::Decryptor::<Aes256>::new_from_slices(key, iv)
                    .map_err(|_| bad_iv)?
                    .decrypt_padded::<NoPadding>(data)
                    .map_err(|_| bad_len)?;
            }
            Cipher::Twofish => {
                cbc::Decryptor::<twofish::Twofish>::new_from_slices(key, iv)
                    .map_err(|_| bad_iv)?
                    .decrypt_padded::<NoPadding>(data)
                    .map_err(|_| bad_len)?;
            }
            Cipher::ChaCha20 => {
                chacha20::ChaCha20::new_from_slices(key, iv)
                    .map_err(|_| bad_iv)?
                    .apply_keystream(data);
            }
        }
        Ok(())
    }

    /// Length of the plaintext once the PKCS#7 padding is removed (block
    /// ciphers only).
    pub fn unpadded_len(self, data: &[u8]) -> Result<usize> {
        if self == Cipher::ChaCha20 {
            return Ok(data.len());
        }
        let bad = Error::Corrupt("bad padding");
        let n = usize::from(*data.last().ok_or(bad)?);
        if n == 0 || n > 16 || n > data.len() {
            return Err(bad);
        }
        if data[data.len() - n..].iter().any(|&b| usize::from(b) != n) {
            return Err(bad);
        }
        Ok(data.len() - n)
    }

    pub fn encrypt(self, key: &[u8; 32], iv: &[u8], data: &[u8]) -> Vec<u8> {
        // key and IV lengths are fixed by this crate, never by the file
        match self {
            Cipher::Aes256 => cbc::Encryptor::<Aes256>::new_from_slices(key, iv)
                .expect("AES key and IV lengths")
                .encrypt_padded_vec::<cipher::block_padding::Pkcs7>(data),
            Cipher::Twofish => cbc::Encryptor::<twofish::Twofish>::new_from_slices(key, iv)
                .expect("Twofish key and IV lengths")
                .encrypt_padded_vec::<cipher::block_padding::Pkcs7>(data),
            Cipher::ChaCha20 => {
                let mut out = data.to_vec();
                chacha20::ChaCha20::new_from_slices(key, iv)
                    .expect("ChaCha20 key and nonce lengths")
                    .apply_keystream(&mut out);
                out
            }
        }
    }
}

// ---------------------------------------------------------------- KDFs

pub const KDF_AES_KDBX3: [u8; 16] = [
    0xc9, 0xd9, 0xf3, 0x9a, 0x62, 0x8a, 0x44, 0x60, 0xbf, 0x74, 0x0d, 0x08, 0xc1, 0x8a, 0x4f, 0xea,
];
pub const KDF_AES_KDBX4: [u8; 16] = [
    0x7c, 0x02, 0xbb, 0x82, 0x79, 0xa7, 0x4a, 0xc0, 0x92, 0x7d, 0x11, 0x4a, 0x00, 0x64, 0x82, 0x38,
];
pub const KDF_ARGON2D: [u8; 16] = [
    0xef, 0x63, 0x6d, 0xdf, 0x8c, 0x29, 0x44, 0x4b, 0x91, 0xf7, 0xa9, 0xa4, 0x03, 0xe3, 0x0a, 0x0c,
];
pub const KDF_ARGON2ID: [u8; 16] = [
    0x9e, 0x29, 0x8b, 0x19, 0x56, 0xdb, 0x47, 0x73, 0xb2, 0x3d, 0xfc, 0x3e, 0xc6, 0xf0, 0xa1, 0xe6,
];

pub fn aes_kdf(composite: &[u8; 32], seed: &[u8], rounds: u64) -> Result<Key32> {
    let cipher =
        Aes256::new_from_slice(seed).map_err(|_| Error::Corrupt("AES-KDF seed is not 32 bytes"))?;
    use zeroize::Zeroize as _;
    let mut blocks = [aes::Block::default(); 2];
    blocks[0].copy_from_slice(&composite[..16]);
    blocks[1].copy_from_slice(&composite[16..]);
    // through the backend: the CPU feature check runs once, not per block
    cipher.encrypt_with_backend(AesKdfRounds {
        blocks: &mut blocks,
        rounds,
    });
    let out = Zeroizing::new(sha256(&[&blocks[0], &blocks[1]]));
    for b in &mut blocks {
        b.as_mut_slice().zeroize();
    }
    Ok(out)
}

pub struct Argon2Params<'a> {
    pub id: bool,
    pub salt: &'a [u8],
    pub memory_bytes: u64,
    pub iterations: u64,
    pub parallelism: u32,
    pub version: u32,
    pub secret: Option<&'a [u8]>,
    pub assoc: Option<&'a [u8]>,
}

pub fn argon2_kdf(composite: &[u8; 32], p: &Argon2Params<'_>) -> Result<Key32> {
    use argon2::{Algorithm, Argon2, AssociatedData, Block, ParamsBuilder, Version};
    let bad = Error::Unsupported("Argon2 parameters");
    let version = match p.version {
        0x10 => Version::V0x10,
        0x13 => Version::V0x13,
        _ => return Err(Error::Unsupported("Argon2 version")),
    };
    let mut b = ParamsBuilder::new();
    b.m_cost(u32::try_from(p.memory_bytes / 1024).map_err(|_| bad)?)
        .t_cost(u32::try_from(p.iterations).map_err(|_| bad)?)
        .p_cost(p.parallelism)
        .output_len(32);
    if let Some(ad) = p.assoc.filter(|a| !a.is_empty()) {
        b.data(AssociatedData::new(ad).map_err(|_| bad)?);
    }
    let params = b.build().map_err(|_| bad)?;
    let alg = if p.id {
        Algorithm::Argon2id
    } else {
        Algorithm::Argon2d
    };
    let argon = match p.secret.filter(|s| !s.is_empty()) {
        Some(s) => Argon2::new_with_secret(s, alg, version, params.clone()).map_err(|_| bad)?,
        None => Argon2::new(alg, version, params.clone()),
    };
    // A damaged or hostile header can ask for any amount of memory. Ask for
    // it up front so that failure is an error, not an abort.
    let mut memory: Vec<Block> = Vec::new();
    memory
        .try_reserve_exact(params.block_count())
        .map_err(|_| Error::OutOfMemory)?;
    memory.resize(params.block_count(), Block::default());
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into_with_memory(composite, p.salt, &mut out[..], &mut memory[..])
        .map_err(|_| bad)?;
    Ok(out)
}

// ---------------------------------------------------------------- inner stream

pub const STREAM_NONE: u32 = 0;
pub const STREAM_SALSA20: u32 = 2;
pub const STREAM_CHACHA20: u32 = 3;

const SALSA20_IV: [u8; 8] = [0xe8, 0x30, 0x09, 0x4b, 0x97, 0x20, 0x5d, 0x2a];

/// The keystream that protects individual values inside the XML.
pub enum InnerStream {
    None,
    Salsa20(Box<salsa20::Salsa20>),
    ChaCha20(Box<chacha20::ChaCha20>),
}

impl InnerStream {
    pub fn new(id: u32, key: &[u8]) -> Result<InnerStream> {
        match id {
            STREAM_NONE => Ok(InnerStream::None),
            STREAM_SALSA20 => {
                let k = Zeroizing::new(sha256(&[key]));
                let s = salsa20::Salsa20::new_from_slices(&k[..], &SALSA20_IV)
                    .map_err(|_| Error::Corrupt("inner stream key"))?;
                Ok(InnerStream::Salsa20(Box::new(s)))
            }
            STREAM_CHACHA20 => {
                let h = sha512(&[key]);
                let s = chacha20::ChaCha20::new_from_slices(&h[..32], &h[32..44])
                    .map_err(|_| Error::Corrupt("inner stream key"))?;
                Ok(InnerStream::ChaCha20(Box::new(s)))
            }
            _ => Err(Error::Unsupported("inner stream cipher")),
        }
    }

    pub fn apply(&mut self, data: &mut [u8]) {
        match self {
            InnerStream::None => {}
            InnerStream::Salsa20(s) => s.apply_keystream(data),
            InnerStream::ChaCha20(s) => s.apply_keystream(data),
        }
    }
}

/// The AES-KDF loop, run inside the cipher backend.
struct AesKdfRounds<'a> {
    blocks: &'a mut [aes::Block; 2],
    rounds: u64,
}

impl cipher::BlockSizeUser for AesKdfRounds<'_> {
    type BlockSize = cipher::consts::U16;
}

impl cipher::BlockCipherEncClosure for AesKdfRounds<'_> {
    fn call<B: cipher::BlockCipherEncBackend<BlockSize = Self::BlockSize>>(self, backend: &B) {
        let [a, b] = self.blocks;
        for _ in 0..self.rounds {
            // two independent chains: the CPU overlaps them
            backend.encrypt_block(a.into());
            backend.encrypt_block(b.into());
        }
    }
}

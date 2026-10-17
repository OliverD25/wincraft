//! SHA-256 through Windows CNG, to check a downloaded update against the
//! checksum the release workflow published next to it.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use windows_sys::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
    BCryptHashData, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE,
    BCRYPT_SHA256_ALGORITHM,
};

const LEN: usize = 32;

struct Hasher {
    algorithm: BCRYPT_ALG_HANDLE,
    hash: BCRYPT_HASH_HANDLE,
}

impl Hasher {
    fn new() -> Result<Self, String> {
        let mut algorithm: BCRYPT_ALG_HANDLE = std::ptr::null_mut();
        let status = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_SHA256_ALGORITHM,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            return Err(format!(
                "SHA-256 is not available (0x{:08X})",
                status as u32
            ));
        }
        let mut hasher = Self {
            algorithm,
            hash: std::ptr::null_mut(),
        };
        // A null object buffer lets CNG allocate the hash state itself.
        let status = unsafe {
            BCryptCreateHash(
                algorithm,
                &mut hasher.hash,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
                0,
                0,
            )
        };
        if status < 0 {
            return Err(format!(
                "cannot start a SHA-256 hash (0x{:08X})",
                status as u32
            ));
        }
        Ok(hasher)
    }

    fn add(&mut self, bytes: &[u8]) -> Result<(), String> {
        for chunk in bytes.chunks(u32::MAX as usize) {
            let status =
                unsafe { BCryptHashData(self.hash, chunk.as_ptr(), chunk.len() as u32, 0) };
            if status < 0 {
                return Err(format!("SHA-256 failed (0x{:08X})", status as u32));
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<[u8; LEN], String> {
        let mut digest = [0u8; LEN];
        let status = unsafe { BCryptFinishHash(self.hash, digest.as_mut_ptr(), LEN as u32, 0) };
        if status < 0 {
            return Err(format!("SHA-256 failed (0x{:08X})", status as u32));
        }
        Ok(digest)
    }
}

impl Drop for Hasher {
    fn drop(&mut self) {
        unsafe {
            if !self.hash.is_null() {
                BCryptDestroyHash(self.hash);
            }
            BCryptCloseAlgorithmProvider(self.algorithm, 0);
        }
    }
}

fn hex(digest: &[u8; LEN]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The checksum of `bytes`, in lower-case hex. Only tests hash bytes in
/// memory; the updater always hashes a file.
#[cfg(test)]
pub fn of_bytes(bytes: &[u8]) -> Result<String, String> {
    let mut hasher = Hasher::new()?;
    hasher.add(bytes)?;
    hasher.finish().map(|digest| hex(&digest))
}

/// The checksum of a file, read in pieces, in lower-case hex.
pub fn of_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut hasher = Hasher::new()?;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.add(&buffer[..read])?;
    }
    hasher.finish().map(|digest| hex(&digest))
}

/// The checksum a `SHA256SUMS.txt` gives for `name`, in lower-case hex. Each
/// line is `<hash>  <name>`; sha256sum marks a binary file with `*`.
pub fn expected(sums: &str, name: &str) -> Option<String> {
    crate::core::config::strip_bom(sums)
        .lines()
        .find_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let file = parts.next()?.trim_start_matches('*');
            let valid = hash.len() == LEN * 2 && hash.chars().all(|ch| ch.is_ascii_hexdigit());
            (valid && file.eq_ignore_ascii_case(name)).then(|| hash.to_ascii_lowercase())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors_hash_to_the_published_values() {
        assert_eq!(
            of_bytes(b"").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            of_bytes(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            of_bytes(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq").unwrap(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn a_file_hashes_the_same_as_its_bytes_across_read_pieces() {
        let path = std::env::temp_dir().join(format!("wincraft-sha-{}.bin", std::process::id()));
        let bytes: Vec<u8> = (0..200_000u32).map(|n| (n % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(of_file(&path).unwrap(), of_bytes(&bytes).unwrap());
        let _ = std::fs::remove_file(&path);
        assert!(of_file(&path).is_err());
    }

    #[test]
    fn the_sums_file_gives_the_hash_for_the_named_file() {
        let hash = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let sums = format!("{hash}  wincraft.exe\r\n");
        assert_eq!(expected(&sums, "wincraft.exe").as_deref(), Some(hash));
        let marked = format!(
            "\u{feff}0000  other.txt\n{}  *WinCraft.exe\n",
            hash.to_uppercase()
        );
        assert_eq!(expected(&marked, "wincraft.exe").as_deref(), Some(hash));
        assert_eq!(expected(&sums, "SHA256SUMS.txt"), None);
        assert_eq!(expected("abc  wincraft.exe", "wincraft.exe"), None);
        assert_eq!(expected("", "wincraft.exe"), None);
    }
}

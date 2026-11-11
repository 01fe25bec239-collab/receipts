//! Content-addressed retained output in a trusted project directory.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// The root must be private to trusted composition; worker access is forbidden.
/// Returned digest covers retained bytes, not an unretained full stream.
pub fn store_output(root: &Path, bytes: &[u8]) -> io::Result<(String, PathBuf)> {
    if bytes.len() > 1_048_576 {
        return Err(io::Error::other("retained output exceeds bound"));
    }
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let path = root.join(&digest);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::File::open(root)?.sync_all()?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::symlink_metadata(&path)?.is_file() {
                return Err(io::Error::other("output is not a regular file"));
            }
            let mut existing = Vec::new();
            fs::File::open(&path)?
                .take(1_048_577)
                .read_to_end(&mut existing)?;
            if existing != bytes {
                return Err(io::Error::other(
                    "content-address collision or incomplete write",
                ));
            }
            fs::File::open(&path)?.sync_all()?;
            fs::File::open(root)?.sync_all()?;
        }
        Err(error) => return Err(error),
    }
    Ok((digest, path))
}

#[cfg(test)]
mod tests {
    #[test]
    fn stores_replays_and_refuses_corruption() {
        let root = crate::test_support::TempDir::new("outputs");
        let (digest, path) = super::store_output(root.path(), b"abc").unwrap();
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        super::store_output(root.path(), b"abc").unwrap();
        std::fs::write(path, b"bad").unwrap();
        assert!(super::store_output(root.path(), b"abc").is_err());
    }
}

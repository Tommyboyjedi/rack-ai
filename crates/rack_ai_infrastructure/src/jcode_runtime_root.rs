use std::fs::{self, DirBuilder, File};
use std::io::{self, Read};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

// Linux runtime paths must not inherit an arbitrarily long client TMPDIR.
const RUNTIME_PREFIX: &str = "/tmp/rack-ai-jcode-run-";
const RANDOM_BYTES: usize = 16;
pub(super) const SOCKET_NAME: &str = "selected-vllm.sock";
pub(super) const MAX_SOCKET_PATH_BYTES: usize =
    RUNTIME_PREFIX.len() + RANDOM_BYTES * 2 + 1 + SOCKET_NAME.len();

pub(super) struct JCodeRuntimeRoot(PathBuf);

impl JCodeRuntimeRoot {
    pub(super) fn create() -> io::Result<Self> {
        let mut random = [0_u8; RANDOM_BYTES];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = PathBuf::from(format!("{RUNTIME_PREFIX}{token}"));
        // Atomic exclusive creation: never reuse another execution's directory.
        DirBuilder::new().mode(0o700).create(&path)?;
        debug_assert_eq!(
            path.join(SOCKET_NAME).as_os_str().len(),
            MAX_SOCKET_PATH_BYTES
        );
        Ok(Self(path))
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for JCodeRuntimeRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

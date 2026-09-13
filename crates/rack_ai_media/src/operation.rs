use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
    time::{Duration, Instant},
};
const OPERATION_WAIT: Duration = Duration::from_secs(20);
const OPERATION_POLL: Duration = Duration::from_millis(20);
pub fn lock(root: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("lifecycle.lock"))
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + OPERATION_WAIT;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(OPERATION_POLL)
            }
            Err(TryLockError::WouldBlock) => {
                return Err("media control operation busy; retry".into());
            }
            Err(TryLockError::Error(error)) => return Err(error.to_string()),
        }
    }
}

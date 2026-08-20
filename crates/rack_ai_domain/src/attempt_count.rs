use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttemptCount(u32);

impl AttemptCount {
    pub fn zero() -> Self {
        Self(0)
    }

    pub fn incremented(&self) -> Self {
        Self(self.0 + 1)
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::AttemptCount;

    #[test]
    fn starts_at_zero() {
        assert_eq!(AttemptCount::zero().value(), 0);
    }

    #[test]
    fn increments_cleanly() {
        let count = AttemptCount::zero().incremented();
        assert_eq!(count.value(), 1);
    }
}

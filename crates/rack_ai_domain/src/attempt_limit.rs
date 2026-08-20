#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptLimit(u32);

impl AttemptLimit {
    pub fn new(value: u32) -> Result<Self, String> {
        if value == 0 {
            return Err("attempt limit must be greater than zero".to_string());
        }

        Ok(Self(value))
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::AttemptLimit;

    #[test]
    fn rejects_zero_attempt_limit() {
        let result = AttemptLimit::new(0);
        assert_eq!(
            result,
            Err("attempt limit must be greater than zero".to_string())
        );
    }

    #[test]
    fn accepts_positive_attempt_limit() {
        let limit = AttemptLimit::new(3).unwrap();
        assert_eq!(limit.value(), 3);
    }
}

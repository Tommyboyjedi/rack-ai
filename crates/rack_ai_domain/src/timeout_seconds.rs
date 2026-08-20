use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimeoutSeconds(u32);

impl TimeoutSeconds {
    pub fn new(value: u32) -> Result<Self, String> {
        if value == 0 {
            return Err("timeout seconds must be greater than zero".to_string());
        }
        Ok(Self(value))
    }

    pub fn value(&self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::TimeoutSeconds;

    #[test]
    fn rejects_zero_timeout() {
        let result = TimeoutSeconds::new(0);
        assert_eq!(
            result,
            Err("timeout seconds must be greater than zero".to_string())
        );
    }

    #[test]
    fn accepts_positive_timeout() {
        let timeout = TimeoutSeconds::new(900).unwrap();
        assert_eq!(timeout.value(), 900);
    }
}

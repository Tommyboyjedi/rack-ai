#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootstrapMessage;

impl BootstrapMessage {
    pub fn value(&self) -> &str {
        "rack-ai infrastructure bootstrap"
    }
}

#[cfg(test)]
mod tests {
    use super::BootstrapMessage;

    #[test]
    fn exposes_bootstrap_message() {
        let message = BootstrapMessage;
        assert_eq!(message.value(), "rack-ai infrastructure bootstrap");
    }
}

use serde::Deserialize;
use serde::Serialize;

use crate::AttemptLimit;
use crate::NetworkPolicy;
use crate::TimeoutSeconds;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeLimits {
    max_implementation_attempts: AttemptLimit,
    timeout_seconds: TimeoutSeconds,
    network: NetworkPolicy,
}

impl ChangeLimits {
    pub fn new(max_implementation_attempts: AttemptLimit, timeout_seconds: TimeoutSeconds) -> Self {
        Self {
            max_implementation_attempts,
            timeout_seconds,
            network: NetworkPolicy::Disabled,
        }
    }

    pub fn with_network(mut self, network: NetworkPolicy) -> Self {
        self.network = network;
        self
    }

    pub fn max_implementation_attempts(&self) -> AttemptLimit {
        self.max_implementation_attempts
    }

    pub fn timeout_seconds(&self) -> TimeoutSeconds {
        self.timeout_seconds
    }

    pub fn network(&self) -> NetworkPolicy {
        self.network
    }
}

#[cfg(test)]
mod tests {
    use super::ChangeLimits;
    use crate::AttemptLimit;
    use crate::NetworkPolicy;
    use crate::TimeoutSeconds;

    #[test]
    fn defaults_to_disabled_network() {
        let limits = ChangeLimits::new(
            AttemptLimit::new(2).unwrap(),
            TimeoutSeconds::new(900).unwrap(),
        );
        assert_eq!(limits.max_implementation_attempts().value(), 2);
        assert_eq!(limits.timeout_seconds().value(), 900);
        assert_eq!(limits.network(), NetworkPolicy::Disabled);
    }
}

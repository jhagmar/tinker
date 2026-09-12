//! Workspace agent that stores the commit chain and talks Mixtrapi frames over TCP.

/// Protocol id the agent speaks on its TCP listener.
pub const PROTOCOL_ID: &str = "mixtrapi/1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_id_is_mixtrapi_1() {
        assert_eq!(PROTOCOL_ID, "mixtrapi/1");
    }
}

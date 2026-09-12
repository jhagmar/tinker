//! Mixtrapi HTTP and session frame types. Later slices add serde shapes and JS codegen.

/// Protocol id on the session WebSocket and in Hello.
pub const PROTOCOL_ID: &str = "mixtrapi/1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_id_is_mixtrapi_1() {
        assert_eq!(PROTOCOL_ID, "mixtrapi/1");
    }
}

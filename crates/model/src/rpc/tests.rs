use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{decode, encode};
use crate::server::ErrorCode;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct Payload {
    value: u8,
}

/// A payload whose serializer always fails, standing in for an unencodable response.
struct Unencodable;

impl Serialize for Unencodable {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("unencodable"))
    }
}

#[test]
fn decodes_matching_parameters_and_rejects_type_or_shape_mismatches() {
    assert_eq!(
        decode::<Payload>(json!({"value":7})),
        Ok(Payload { value: 7 })
    );
    assert_eq!(
        decode::<Payload>(json!({"value":"seven"})),
        Err(ErrorCode::InvalidMessage)
    );
    // Unknown fields stay ignored unless the request type denies them.
    assert_eq!(
        decode::<Payload>(json!({"value":7,"future":true})),
        Ok(Payload { value: 7 })
    );
    assert_eq!(
        decode::<Payload>(json!(null)),
        Err(ErrorCode::InvalidMessage)
    );
}

#[test]
fn encodes_payloads_and_reports_the_requested_failure_code() {
    assert_eq!(
        encode(Payload { value: 3 }, ErrorCode::ProjectIo),
        Ok(json!({"value":3}))
    );
    assert_eq!(
        encode(Unencodable, ErrorCode::ProjectIo),
        Err(ErrorCode::ProjectIo)
    );
    assert_eq!(
        encode(Unencodable, ErrorCode::RegistryIo),
        Err(ErrorCode::RegistryIo)
    );
}

#[test]
fn encode_rejects_map_keys_that_json_cannot_represent() {
    let mut map = BTreeMap::new();
    map.insert(vec![1_u8], 1_u8);
    assert_eq!(
        encode(map, ErrorCode::InvalidMessage),
        Err(ErrorCode::InvalidMessage)
    );
}

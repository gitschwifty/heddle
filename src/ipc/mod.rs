//! IPC types, codec, error normalization, protocol versioning.

pub mod codec;
pub mod errors;
pub mod protocol;
pub mod schema;
pub mod types;

pub use codec::{
    build_error, build_result, decode_request, encode_response, wrap_event, BuildResultArgs,
    CorrelationContext,
};
pub use errors::{normalize_error, ErrorEnvelope, NormalizedError};
pub use protocol::{
    check_compatibility, parse_semver, CompatLevel, CompatResult, PROTOCOL_VERSION,
};
pub use schema::validate_ipc_message;
pub use types::*;

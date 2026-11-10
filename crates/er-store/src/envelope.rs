use serde::Serialize;
use serde_json::Value;

use crate::artifact::{Artifact, Mode};
use crate::error::StoreError;

/// The wrapper fields a `Found` file carried, when it was enveloped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvelopeMeta {
    pub version: u32,
    pub diff_hash: Option<String>,
    pub producer: Option<String>,
    pub written_at: Option<String>,
}

/// A value the store found on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Stored<T> {
    pub value: T,
    pub meta: Option<EnvelopeMeta>,
}

/// Why a file exists but cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unreadable {
    Malformed(String),
    UnknownVersion { found: u32 },
    NewerVersion { found: u32 },
    WrongKind { found: String },
    TooLarge { bytes: u64 },
}

/// The result of reading one artifact.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded<T> {
    Absent,
    Found(Stored<T>),
    Unreadable(Unreadable),
}

/// The borrowed write form of the envelope. A struct keeps the field order
/// stable; `serde_json::Value` would sort the keys.
#[derive(Serialize)]
pub(crate) struct Envelope<'a, T> {
    pub kind: &'static str,
    pub version: u32,
    pub diff_hash: Option<&'a str>,
    pub producer: Option<&'static str>,
    pub written_at: String,
    pub payload: &'a T,
}

/// Parse a JSON document as one artifact kind.
///
/// Public so a caller holding a captured copy of a file (a stamped overlay)
/// can apply the same version and shape rules as a store read.
pub fn parse<T: Artifact>(value: Value) -> Loaded<T> {
    match T::MODE {
        Mode::Enveloped => parse_enveloped::<T>(value),
        Mode::Bare => parse_bare::<T>(value),
    }
}

fn parse_enveloped<T: Artifact>(value: Value) -> Loaded<T> {
    let Some(object) = value.as_object() else {
        return malformed("not a JSON object");
    };
    let has_kind = object.contains_key("kind");
    let has_payload = object.contains_key("payload");
    if !has_kind && !has_payload {
        return match T::WAS_BARE_BEFORE {
            Some(max_bare) => parse_legacy_bare::<T>(value, max_bare),
            None => malformed("missing envelope"),
        };
    }
    if !has_kind || !has_payload {
        return malformed("missing envelope field");
    }
    let Some(kind) = object.get("kind").and_then(Value::as_str) else {
        return malformed("kind is not a string");
    };
    if kind != T::KIND {
        return Loaded::Unreadable(Unreadable::WrongKind {
            found: kind.to_string(),
        });
    }
    let Some(version) = version_of(&value) else {
        return malformed("missing version");
    };
    if version > T::VERSION {
        return Loaded::Unreadable(Unreadable::NewerVersion { found: version });
    }
    let Some(payload) = object.get("payload").cloned() else {
        return malformed("missing payload");
    };
    let meta = EnvelopeMeta {
        version,
        diff_hash: object
            .get("diff_hash")
            .and_then(Value::as_str)
            .map(str::to_owned),
        producer: object
            .get("producer")
            .and_then(Value::as_str)
            .map(str::to_owned),
        written_at: object
            .get("written_at")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    if version < T::VERSION {
        return migrate::<T>(version, payload, Some(meta));
    }
    found::<T>(payload, Some(meta))
}

fn parse_bare<T: Artifact>(value: Value) -> Loaded<T> {
    let Some(object) = value.as_object() else {
        return malformed("not a JSON object");
    };
    let has_kind = object.contains_key("kind");
    let has_payload = object.contains_key("payload");
    if has_kind && has_payload {
        let Some(kind) = object.get("kind").and_then(Value::as_str) else {
            return malformed("kind is not a string");
        };
        if let Some(found) = newer_envelope_version::<T>(kind, &value) {
            return Loaded::Unreadable(Unreadable::NewerVersion { found });
        }
        return Loaded::Unreadable(Unreadable::WrongKind {
            found: kind.to_string(),
        });
    }
    if has_payload {
        return malformed("envelope payload in a bare kind");
    }
    parse_bare_payload::<T>(value, None)
}

/// A `Bare` kind that finds its own envelope with a newer version must not
/// overwrite it. Any other envelope is the wrong shape for a bare file.
fn newer_envelope_version<T: Artifact>(kind: &str, value: &Value) -> Option<u32> {
    if kind != T::KIND {
        return None;
    }
    version_of(value).filter(|version| *version > T::VERSION)
}

fn parse_legacy_bare<T: Artifact>(value: Value, max_bare: u32) -> Loaded<T> {
    let Some(version) = version_of(&value) else {
        return malformed("missing version");
    };
    if version > T::VERSION {
        return Loaded::Unreadable(Unreadable::NewerVersion { found: version });
    }
    if version > max_bare {
        return malformed("bare version is newer than the bare mode allowed");
    }
    parse_bare_payload::<T>(value, None)
}

fn parse_bare_payload<T: Artifact>(value: Value, meta: Option<EnvelopeMeta>) -> Loaded<T> {
    let Some(version) = version_of(&value) else {
        return malformed("missing version");
    };
    if version > T::VERSION {
        return Loaded::Unreadable(Unreadable::NewerVersion { found: version });
    }
    if version < T::VERSION {
        return migrate::<T>(version, value, meta);
    }
    found::<T>(value, meta)
}

fn version_of(value: &Value) -> Option<u32> {
    value
        .get("version")
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
}

fn migrate<T: Artifact>(from: u32, raw: Value, meta: Option<EnvelopeMeta>) -> Loaded<T> {
    match T::migrate(from, raw) {
        Ok(value) => found::<T>(value, meta),
        Err(StoreError::UnsupportedVersion { found }) => {
            Loaded::Unreadable(Unreadable::UnknownVersion { found })
        }
        Err(error) => Loaded::Unreadable(Unreadable::Malformed(error.to_string())),
    }
}

fn found<T: Artifact>(value: Value, meta: Option<EnvelopeMeta>) -> Loaded<T> {
    match serde_json::from_value(value) {
        Ok(value) => Loaded::Found(Stored { value, meta }),
        Err(error) => Loaded::Unreadable(Unreadable::Malformed(error.to_string())),
    }
}

fn malformed<T>(reason: impl Into<String>) -> Loaded<T> {
    Loaded::Unreadable(Unreadable::Malformed(reason.into()))
}

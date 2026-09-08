//! Private decoders for the A2 source registry. No generic document survives.
use super::*;
use serde::{
    Deserialize, Deserializer,
    de::{IgnoredAny, MapAccess, Visitor},
};

// Serde structs also accept positional sequences by default. Vendor objects
// must be maps, including empty objects with only optional fields.
struct Object<T>(T);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct MapOnly<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for MapOnly<T> {
            type Value = Object<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("source object")
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                T::deserialize(serde::de::value::MapAccessDeserializer::new(map)).map(Object)
            }
        }
        d.deserialize_map(MapOnly(std::marker::PhantomData))
    }
}

#[derive(Default)]
enum Field<T> {
    #[default]
    Missing,
    Null,
    Present(T),
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Field<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(d)? {
            Some(v) => Self::Present(v),
            None => Self::Null,
        })
    }
}
impl<T> Field<T> {
    fn observed(self) -> Value<T> {
        match self {
            Self::Missing => Value::Absent,
            Self::Null => Value::Unknown,
            Self::Present(v) => Value::Known(v),
        }
    }
}

fn text(bytes: &[u8]) -> Result<&str, Error> {
    if bytes.len() > HOST_STRUCTURED_OBSERVATION_MAX_BYTES {
        return Err(Error::StructuredObservationTooLarge);
    }
    std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)
}

#[derive(Deserialize)]
struct ClaudePlugin {
    id: String,
    #[serde(default)]
    version: Field<String>,
    #[serde(default)]
    enabled: Field<bool>,
}

fn is_receipts(id: &str) -> bool {
    id == "receipts"
        || id
            .strip_prefix("receipts@")
            .is_some_and(|source| !source.is_empty())
}

pub(super) fn claude_plugins(bytes: &[u8]) -> Result<Vec<HostObservedPlugin>, Error> {
    // Actual installed C1 shape verified before implementation: array of objects;
    // id is required. version/enabled are optional facts, not default values.
    let rows: Vec<Object<ClaudePlugin>> =
        serde_json::from_str(text(bytes)?).map_err(|_| Error::InvalidJson)?;
    if rows.iter().any(|row| row.0.id.is_empty()) {
        return Err(Error::InvalidJson);
    }
    Ok(rows
        .into_iter()
        .map(|row| row.0)
        .filter(|row| is_receipts(&row.id))
        .map(|row| HostObservedPlugin {
            id: row.id,
            version: row.version.observed(),
            enabled: row.enabled.observed(),
        })
        .collect())
}

// A source-specific map visitor discards unrelated plugin entries as IgnoredAny.
// Duplicate Receipts keys (including a previous null) fail the entire source.
struct ReceiptsSettings(Vec<HostObservedPluginSetting>);
impl<'de> Deserialize<'de> for ReceiptsSettings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> Visitor<'de> for Entries {
            type Value = ReceiptsSettings;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("enabledPlugins object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries: Vec<HostObservedPluginSetting> = Vec::new();
                let mut seen = std::collections::BTreeSet::new();
                while let Some(id) = map.next_key::<String>()? {
                    if is_receipts(&id) {
                        if !seen.insert(id.clone()) {
                            return Err(serde::de::Error::custom("duplicate Receipts setting"));
                        }
                        let enabled = map.next_value::<Field<bool>>()?.observed();
                        entries.push(HostObservedPluginSetting { id, enabled });
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(ReceiptsSettings(entries))
            }
        }
        d.deserialize_map(Entries)
    }
}

/// Handler bodies, commands, matcher text, and prompts are discarded. A
/// nonempty handler list proves configuration presence, never executability.
#[derive(Deserialize)]
struct HookGroup {
    hooks: Vec<Object<HookHandler>>,
}

#[derive(Deserialize)]
struct HookHandler {}

// This is deliberately a named set, not a vendor-key map. Unknown event names
// cannot carry arbitrary source text into observation output. It is NOT an
// exhaustive vendor surface or the orchestrator's required lifecycle set.
#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct ConfiguredEvents {
    #[serde(default)]
    session_start: Field<Vec<Object<HookGroup>>>,
    #[serde(default)]
    session_end: Field<Vec<Object<HookGroup>>>,
    #[serde(default)]
    user_prompt_submit: Field<Vec<Object<HookGroup>>>,
    #[serde(default)]
    pre_tool_use: Field<Vec<Object<HookGroup>>>,
    #[serde(default)]
    post_tool_use: Field<Vec<Object<HookGroup>>>,
    #[serde(default)]
    stop: Field<Vec<Object<HookGroup>>>,
}
impl ConfiguredEvents {
    fn names(self) -> Value<Vec<String>> {
        let fields = [
            ("SessionStart", self.session_start),
            ("SessionEnd", self.session_end),
            ("UserPromptSubmit", self.user_prompt_submit),
            ("PreToolUse", self.pre_tool_use),
            ("PostToolUse", self.post_tool_use),
            ("Stop", self.stop),
        ];
        // Null is not a known-empty event configuration. Do not partially
        // project this list as complete evidence when one known event is null.
        if fields.iter().any(|(_, field)| matches!(field, Field::Null)) {
            return Value::Unknown;
        }
        Value::Known(
            fields
                .into_iter()
                .filter_map(|(name, field)| match field {
                    Field::Present(groups)
                        if groups.iter().any(|group| !group.0.hooks.is_empty()) =>
                    {
                        Some(name.to_owned())
                    }
                    _ => None,
                })
                .collect(),
        )
    }
}
fn event_names(field: Field<Object<ConfiguredEvents>>) -> Value<Vec<String>> {
    match field {
        Field::Missing => Value::Absent,
        Field::Null => Value::Unknown,
        Field::Present(events) => events.0.names(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSettings {
    #[serde(default)]
    disable_all_hooks: Field<bool>,
    #[serde(default)]
    allow_managed_hooks_only: Field<bool>,
    #[serde(default)]
    enabled_plugins: Field<ReceiptsSettings>,
    #[serde(default)]
    hooks: Field<Object<ConfiguredEvents>>,
}

#[derive(Deserialize)]
struct CodexHooks {
    hooks: Object<ConfiguredEvents>,
}

#[derive(Deserialize)]
struct CodexFeatures {
    #[serde(default)]
    hooks: Field<bool>,
}

#[derive(Deserialize)]
struct CodexConfig {
    #[serde(default)]
    features: Field<Object<CodexFeatures>>,
    #[serde(default)]
    allow_managed_hooks_only: Field<bool>,
    // TOML [hooks] is not treated as the JSON event-handler shape. Without an
    // independently established typed shape it is ignored, never guessed.
}

pub(super) fn configuration(
    source: Source,
    bytes: &[u8],
) -> Result<HostObservedConfiguration, Error> {
    let input = text(bytes)?;
    match source {
        Source::ClaudeUserSettings
        | Source::ClaudeProjectSettings
        | Source::ClaudeLocalSettings
        | Source::ClaudeManagedSettings
        | Source::ClaudeManagedDropIn => {
            let Object(settings): Object<ClaudeSettings> =
                serde_json::from_str(input).map_err(|_| Error::InvalidJson)?;
            let receipts_enabled = match settings.enabled_plugins {
                Field::Missing => Value::Absent,
                Field::Null => Value::Unknown,
                Field::Present(settings) => Value::Known(settings.0),
            };
            Ok(HostObservedConfiguration {
                disable_all_hooks: settings.disable_all_hooks.observed(),
                allow_managed_hooks_only: if matches!(
                    source,
                    Source::ClaudeManagedSettings | Source::ClaudeManagedDropIn
                ) {
                    settings.allow_managed_hooks_only.observed()
                } else {
                    Value::Unknown
                },
                receipts_enabled,
                configured_recognized_events: event_names(settings.hooks),
                ..Default::default()
            })
        }
        Source::CodexUserHooks | Source::CodexProjectHooks => {
            let Object(hooks): Object<CodexHooks> =
                serde_json::from_str(input).map_err(|_| Error::InvalidJson)?;
            Ok(HostObservedConfiguration {
                configured_recognized_events: hooks.hooks.0.names(),
                ..Default::default()
            })
        }
        Source::CodexUserConfig | Source::CodexProjectConfig | Source::CodexLocalRequirements => {
            let settings: CodexConfig = toml::from_str(input).map_err(|_| Error::InvalidToml)?;
            let hooks_feature = match settings.features {
                Field::Missing => Value::Absent,
                Field::Null => Value::Unknown,
                Field::Present(features) => features.0.hooks.observed(),
            };
            Ok(HostObservedConfiguration {
                hooks_feature,
                allow_managed_hooks_only: if source == Source::CodexLocalRequirements {
                    settings.allow_managed_hooks_only.observed()
                } else {
                    Value::Unknown
                },
                ..Default::default()
            })
        }
        Source::ClaudePluginList => Err(Error::Unsupported),
    }
}

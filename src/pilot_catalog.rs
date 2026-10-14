//! Pilot persona, portrait, and dialogue voice assets.
//!
//! A save's pilot roster stores a copy of an `MWPersonaAsset` (callsign, name,
//! bio, portrait, voice, gameplay tags). Portraits and voices are referenced by
//! primary asset name (`MWPortraitAsset` / `MWDialogueVoiceAsset`). This module
//! extracts all three so an editor can offer the full set of valid choices.

use serde::{Deserialize, Serialize};
use unreal_asset::properties::{Property, PropertyDataTrait};

use crate::{trait_catalog::string_from_property, unreal_name::render_fname};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PilotAssetKind {
    Persona,
    Portrait,
    Voice,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JjPersona {
    pub asset_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portrait: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum JjPilotAsset {
    Persona(JjPersona),
    Portrait(String),
    Voice(String),
}

impl JjPilotAsset {
    pub(crate) fn kind(&self) -> PilotAssetKind {
        match self {
            Self::Persona(_) => PilotAssetKind::Persona,
            Self::Portrait(_) => PilotAssetKind::Portrait,
            Self::Voice(_) => PilotAssetKind::Voice,
        }
    }

    pub fn asset_name(&self) -> &str {
        match self {
            Self::Persona(persona) => &persona.asset_name,
            Self::Portrait(name) | Self::Voice(name) => name,
        }
    }

    /// Identity used to dedupe and merge across sources.
    pub(crate) fn key(&self) -> String {
        format!(
            "{:?}:{}",
            self.kind(),
            self.asset_name().to_ascii_lowercase()
        )
    }
}

pub(crate) fn pilot_asset_kind_from_class(class_name: &str) -> Option<PilotAssetKind> {
    let lower = class_name.to_ascii_lowercase();
    if lower.contains("mwpersonaasset") {
        Some(PilotAssetKind::Persona)
    } else if lower.contains("mwportraitasset") {
        Some(PilotAssetKind::Portrait)
    } else if lower.contains("mwdialoguevoiceasset") {
        Some(PilotAssetKind::Voice)
    } else {
        None
    }
}

pub(crate) fn pilot_asset_from_unreal_export(
    class_name: &str,
    asset_name: &str,
    properties: &[Property],
) -> Option<JjPilotAsset> {
    let kind = pilot_asset_kind_from_class(class_name)?;
    if asset_name.trim().is_empty() || asset_name.eq_ignore_ascii_case("None") {
        return None;
    }
    dump_for_diagnostics(kind, class_name, asset_name, properties);
    Some(match kind {
        PilotAssetKind::Portrait => JjPilotAsset::Portrait(asset_name.to_string()),
        PilotAssetKind::Voice => JjPilotAsset::Voice(asset_name.to_string()),
        PilotAssetKind::Persona => JjPilotAsset::Persona(JjPersona {
            asset_name: asset_name.to_string(),
            callsign: text_by_name(properties, "Callsign"),
            full_name: text_by_name(properties, "FullName"),
            bio: text_by_name(properties, "Bio"),
            portrait: primary_asset_name(properties, "PortraitAssetId"),
            voice: primary_asset_name(properties, "DialogueVoiceAssetId"),
            attributes: tags_by_name(properties, "Attributes"),
        }),
    })
}

/// Diagnostics: when `SCARAB_PILOT_DUMP=<file>` is set, append the raw
/// properties of the first few exports of each pilot asset kind to that file,
/// so catalog extraction can be checked against real game data. Exports named
/// in `SCARAB_PILOT_DUMP_NAMES` (comma separated) are always dumped too.
fn dump_for_diagnostics(
    kind: PilotAssetKind,
    class_name: &str,
    asset_name: &str,
    properties: &[Property],
) {
    use std::{
        io::Write,
        sync::atomic::{AtomicUsize, Ordering},
    };
    const LIMIT: usize = 3;
    static COUNTS: [AtomicUsize; 3] = [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ];
    let Some(path) = std::env::var_os("SCARAB_PILOT_DUMP") else {
        return;
    };
    let named = std::env::var("SCARAB_PILOT_DUMP_NAMES").is_ok_and(|names| {
        names
            .split(',')
            .any(|name| name.trim().eq_ignore_ascii_case(asset_name))
    });
    if COUNTS[kind as usize].fetch_add(1, Ordering::Relaxed) >= LIMIT && !named {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "=== {class_name} / {asset_name}\n{properties:#?}\n");
    }
}

/// Depth-first search for the first property with this exact (case-insensitive) name.
fn find_property<'a>(properties: &'a [Property], name: &str) -> Option<&'a Property> {
    for property in properties {
        if property.get_name().get_content().eq_ignore_ascii_case(name) {
            return Some(property);
        }
        let nested = match property {
            Property::StructProperty(property) => find_property(&property.value, name),
            Property::ArrayProperty(property) => find_property(&property.value, name),
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
    }
    None
}

fn text_by_name(properties: &[Property], name: &str) -> Option<String> {
    find_property(properties, name).and_then(string_from_property)
}

/// `XxxAssetId` properties are `{ ID: PrimaryAssetId { PrimaryAssetType, PrimaryAssetName } }`.
fn primary_asset_name(properties: &[Property], id_property: &str) -> Option<String> {
    let Property::StructProperty(wrapper) = find_property(properties, id_property)? else {
        return None;
    };
    let name = find_property(&wrapper.value, "PrimaryAssetName").and_then(string_from_property)?;
    (!name.eq_ignore_ascii_case("None")).then_some(name)
}

fn tags_by_name(properties: &[Property], name: &str) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(property) = find_property(properties, name) {
        collect_tags(property, &mut tags);
    }
    tags.sort();
    tags.dedup();
    tags
}

/// The tag container is a struct property wrapping a `GameplayTagContainerProperty`
/// of the same name, so look through structs and arrays for the tag names.
fn collect_tags(property: &Property, tags: &mut Vec<String>) {
    match property {
        Property::GameplayTagContainerProperty(property) => {
            tags.extend(property.value.iter().map(render_fname));
        }
        Property::StructProperty(property) => {
            property
                .value
                .iter()
                .for_each(|child| collect_tags(child, tags));
        }
        Property::ArrayProperty(property) => {
            property
                .value
                .iter()
                .for_each(|child| collect_tags(child, tags));
        }
        Property::NameProperty(_) | Property::StrProperty(_) => {
            tags.extend(string_from_property(property));
        }
        _ => {}
    }
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct PilotCatalogJson {
    #[serde(default)]
    pub(crate) personas: Vec<JjPersona>,
    #[serde(default)]
    pub(crate) portraits: Vec<String>,
    #[serde(default)]
    pub(crate) voices: Vec<String>,
}

impl PilotCatalogJson {
    pub(crate) fn into_assets(self) -> Vec<JjPilotAsset> {
        let mut assets = Vec::new();
        assets.extend(self.personas.into_iter().map(JjPilotAsset::Persona));
        assets.extend(self.portraits.into_iter().map(JjPilotAsset::Portrait));
        assets.extend(self.voices.into_iter().map(JjPilotAsset::Voice));
        assets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_pilot_asset_classes() {
        assert_eq!(
            pilot_asset_kind_from_class("MWPersonaAsset"),
            Some(PilotAssetKind::Persona)
        );
        assert_eq!(
            pilot_asset_kind_from_class("MWPortraitAsset"),
            Some(PilotAssetKind::Portrait)
        );
        assert_eq!(
            pilot_asset_kind_from_class("MWDialogueVoiceAsset"),
            Some(PilotAssetKind::Voice)
        );
        assert_eq!(pilot_asset_kind_from_class("MWPilotTraitDataAsset"), None);
    }

    #[test]
    fn portrait_and_voice_use_the_export_name() {
        assert_eq!(
            pilot_asset_from_unreal_export("MWPortraitAsset", "LeonFreeman", &[]),
            Some(JjPilotAsset::Portrait("LeonFreeman".to_string()))
        );
        assert_eq!(
            pilot_asset_from_unreal_export("MWDialogueVoiceAsset", "MaleAlly4", &[]),
            Some(JjPilotAsset::Voice("MaleAlly4".to_string()))
        );
        assert_eq!(
            pilot_asset_from_unreal_export("MWPortraitAsset", "None", &[]),
            None
        );
    }

    #[test]
    fn persona_without_known_properties_still_lists_the_asset() {
        let asset = pilot_asset_from_unreal_export("MWPersonaAsset", "Raptor", &[]).unwrap();
        assert_eq!(
            asset,
            JjPilotAsset::Persona(JjPersona {
                asset_name: "Raptor".to_string(),
                callsign: None,
                full_name: None,
                bio: None,
                portrait: None,
                voice: None,
                attributes: Vec::new(),
            })
        );
    }

    #[test]
    fn catalog_json_round_trips_and_tolerates_missing_sections() {
        let parsed: PilotCatalogJson =
            serde_json::from_str(r#"{"portraits": ["A"], "voices": ["B"]}"#).unwrap();
        let assets = parsed.into_assets();
        assert_eq!(
            assets,
            vec![
                JjPilotAsset::Portrait("A".to_string()),
                JjPilotAsset::Voice("B".to_string())
            ]
        );
        assert_eq!(assets[0].key(), "Portrait:a");
    }

    #[test]
    fn attributes_tags_are_found_inside_a_wrapping_struct() {
        use unreal_asset::{
            properties::{
                gameplay_tag_container_property::GameplayTagContainerProperty,
                struct_property::StructProperty,
            },
            types::fname::FName,
            unversioned::ancestry::Ancestry,
        };
        let container = Property::GameplayTagContainerProperty(GameplayTagContainerProperty {
            name: FName::new_dummy("Attributes".to_string(), 0),
            ancestry: Ancestry::default(),
            property_guid: None,
            duplication_index: 0,
            value: vec![
                FName::new_dummy("PilotPersona.Use.Hero".to_string(), 0),
                FName::new_dummy("DLC.DLC7".to_string(), 0),
            ],
        });
        let wrapper = Property::StructProperty(StructProperty {
            name: FName::new_dummy("Attributes".to_string(), 0),
            ancestry: Ancestry::default(),
            property_guid: None,
            duplication_index: 0,
            struct_type: None,
            struct_guid: None,
            value: vec![container],
            serialize_none: true,
        });
        assert_eq!(
            tags_by_name(&[wrapper], "Attributes"),
            vec!["DLC.DLC7".to_string(), "PilotPersona.Use.Hero".to_string()]
        );
    }
}

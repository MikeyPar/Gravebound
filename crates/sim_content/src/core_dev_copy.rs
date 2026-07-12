//! Strict checked-in UI copy for the unpromoted Core identity client (`GB-M03-01C`).

use std::{collections::BTreeSet, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{ContentId, CoreIdentityCopyFile, CoreIdentityPhaseCopy, SCHEMA_VERSION};

use crate::load_core_development_identity;

pub const CORE_IDENTITY_COPY_PATH: &str = "core_dev/identity.en-US.json";
pub const CORE_IDENTITY_LOCALE: &str = "en-US";
pub const CORE_CLOSED_FEATURE_LITERAL: &str = "AVAILABLE IN A LATER TEST";
pub const CORE_NOT_EQUIPPED_LITERAL: &str = "NOT EQUIPPED";

/// Mechanically sourced localization for one stable ability record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizedCoreAbility {
    id: ContentId,
    name: String,
    description: String,
}

impl LocalizedCoreAbility {
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Validated Core UI copy plus mechanically sourced class/ability localization.
#[derive(Debug, Clone)]
pub struct CoreDevelopmentIdentityCopy {
    copy: CoreIdentityCopyFile,
    class_name: String,
    class_description: String,
    abilities: Vec<LocalizedCoreAbility>,
}

impl CoreDevelopmentIdentityCopy {
    #[must_use]
    pub const fn copy(&self) -> &CoreIdentityCopyFile {
        &self.copy
    }

    #[must_use]
    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    #[must_use]
    pub fn class_description(&self) -> &str {
        &self.class_description
    }

    #[must_use]
    pub fn abilities(&self) -> &[LocalizedCoreAbility] {
        &self.abilities
    }
}

/// Loads all player-visible Core identity copy through strict checked-in content.
pub fn load_core_development_identity_copy(root: &Path) -> Result<CoreDevelopmentIdentityCopy> {
    let identity = load_core_development_identity(root)?;
    let (source, _) = crate::load_and_validate(root)?;
    let copy: CoreIdentityCopyFile = crate::read_json(&root.join(CORE_IDENTITY_COPY_PATH))?;
    validate_copy(&copy)?;

    let class_name = localized(
        &source.localization,
        identity.class().header.localization_name_key.as_str(),
    )?;
    let class_description = localized(
        &source.localization,
        identity
            .class()
            .header
            .localization_description_key
            .as_str(),
    )?;
    let abilities = identity
        .abilities()
        .iter()
        .map(|record| {
            Ok(LocalizedCoreAbility {
                id: record.header.id.clone(),
                name: localized(
                    &source.localization,
                    record.header.localization_name_key.as_str(),
                )?,
                description: localized(
                    &source.localization,
                    record.header.localization_description_key.as_str(),
                )?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(CoreDevelopmentIdentityCopy {
        copy,
        class_name,
        class_description,
        abilities,
    })
}

fn localized(
    localization: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<String> {
    localization
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .with_context(|| format!("Core identity copy references missing localization key {key}"))
}

fn validate_copy(copy: &CoreIdentityCopyFile) -> Result<()> {
    if copy.schema_version != SCHEMA_VERSION {
        bail!("Core identity copy schema version must be {SCHEMA_VERSION}");
    }
    if copy.locale != CORE_IDENTITY_LOCALE {
        bail!("Core identity copy locale must be en-US");
    }
    if copy.closed_feature_literal != CORE_CLOSED_FEATURE_LITERAL {
        bail!("Core identity closed-feature copy must remain the exact approved literal");
    }
    if copy.not_equipped_literal != CORE_NOT_EQUIPPED_LITERAL {
        bail!("Core identity item-power copy must remain the exact approved literal");
    }

    for (field, value) in all_copy_fields(copy) {
        validate_text(field, value)?;
    }
    for (field, value) in literal_copy_fields(copy) {
        require_placeholders(field, value, &[])?;
    }
    require_placeholders(
        "status_template",
        &copy.status_template,
        &["error", "feature_flag", "major", "minor", "phase"],
    )?;
    require_placeholders(
        "populated_slot_template",
        &copy.populated_slot_template,
        &["class_name", "level", "not_equipped", "ordinal", "selected"],
    )?;
    require_placeholders(
        "empty_slot_template",
        &copy.empty_slot_template,
        &["class_name", "ordinal"],
    )?;
    require_placeholders(
        "class_detail_template",
        &copy.class_detail_template,
        &["class_name", "not_equipped", "unavailable"],
    )?;
    require_placeholders(
        "select_slot_action_template",
        &copy.select_slot_action_template,
        &["ordinal"],
    )?;
    require_placeholders("footer_template", &copy.footer_template, &["unavailable"])?;
    Ok(())
}

fn all_copy_fields(copy: &CoreIdentityCopyFile) -> Vec<(&'static str, &str)> {
    let mut fields = vec![
        ("locale", copy.locale.as_str()),
        ("window_title", copy.window_title.as_str()),
        ("brand_header", copy.brand_header.as_str()),
        ("wipe_warning", copy.wipe_warning.as_str()),
        ("status_template", copy.status_template.as_str()),
        ("loading_roster", copy.loading_roster.as_str()),
        (
            "populated_slot_template",
            copy.populated_slot_template.as_str(),
        ),
        ("empty_slot_template", copy.empty_slot_template.as_str()),
        ("selected_badge", copy.selected_badge.as_str()),
        ("class_detail_template", copy.class_detail_template.as_str()),
        ("create_action", copy.create_action.as_str()),
        (
            "select_slot_action_template",
            copy.select_slot_action_template.as_str(),
        ),
        ("retry_action", copy.retry_action.as_str()),
        ("footer_template", copy.footer_template.as_str()),
        (
            "closed_feature_literal",
            copy.closed_feature_literal.as_str(),
        ),
        ("not_equipped_literal", copy.not_equipped_literal.as_str()),
    ];
    fields.extend(phase_fields(&copy.phases));
    fields
}

fn literal_copy_fields(copy: &CoreIdentityCopyFile) -> Vec<(&'static str, &str)> {
    let mut fields = vec![
        ("locale", copy.locale.as_str()),
        ("window_title", copy.window_title.as_str()),
        ("brand_header", copy.brand_header.as_str()),
        ("wipe_warning", copy.wipe_warning.as_str()),
        ("loading_roster", copy.loading_roster.as_str()),
        ("selected_badge", copy.selected_badge.as_str()),
        ("create_action", copy.create_action.as_str()),
        ("retry_action", copy.retry_action.as_str()),
        (
            "closed_feature_literal",
            copy.closed_feature_literal.as_str(),
        ),
        ("not_equipped_literal", copy.not_equipped_literal.as_str()),
    ];
    fields.extend(phase_fields(&copy.phases));
    fields
}

fn phase_fields(phases: &CoreIdentityPhaseCopy) -> [(&'static str, &str); 13] {
    [
        ("phases.boot", &phases.boot),
        ("phases.patch_check", &phases.patch_check),
        ("phases.authenticating", &phases.authenticating),
        ("phases.roster_loading", &phases.roster_loading),
        ("phases.roster_empty", &phases.roster_empty),
        ("phases.roster_ready", &phases.roster_ready),
        ("phases.character_creation", &phases.character_creation),
        ("phases.creating", &phases.creating),
        ("phases.selecting", &phases.selecting),
        ("phases.selected", &phases.selected),
        ("phases.disconnected", &phases.disconnected),
        ("phases.disabled", &phases.disabled),
        ("phases.error", &phases.error),
    ]
}

fn validate_text(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 2_048 {
        bail!("Core identity copy field {field} is empty or exceeds 2048 bytes");
    }
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n')
    {
        bail!("Core identity copy field {field} contains a disallowed control character");
    }
    Ok(())
}

fn require_placeholders(field: &str, template: &str, expected: &[&str]) -> Result<()> {
    let actual = template_placeholders(template)
        .with_context(|| format!("Core identity copy field {field} has malformed placeholders"))?;
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        bail!("Core identity copy field {field} has an unauthorized placeholder set");
    }
    Ok(())
}

fn template_placeholders(template: &str) -> Result<BTreeSet<&str>> {
    let mut placeholders = BTreeSet::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        if rest[..open].contains('}') {
            bail!("closing brace precedes opening brace");
        }
        let after_open = &rest[open + 1..];
        let close = after_open.find('}').context("unclosed placeholder")?;
        let placeholder = &after_open[..close];
        if placeholder.is_empty()
            || placeholder
                .chars()
                .any(|character| !character.is_ascii_lowercase() && character != '_')
        {
            bail!("invalid placeholder name");
        }
        placeholders.insert(placeholder);
        rest = &after_open[close + 1..];
    }
    if rest.contains('}') {
        bail!("unmatched closing brace");
    }
    Ok(placeholders)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn content_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn checked_in_copy() -> CoreIdentityCopyFile {
        crate::read_json(&content_root().join(CORE_IDENTITY_COPY_PATH)).expect("checked-in copy")
    }

    #[test]
    fn checked_in_copy_and_mechanical_localization_are_complete() {
        let compiled = load_core_development_identity_copy(&content_root()).expect("valid copy");
        assert_eq!(compiled.copy().locale, "en-US");
        assert_eq!(compiled.class_name(), "Grave Arbalist");
        assert!(!compiled.class_description().is_empty());
        assert_eq!(compiled.abilities().len(), 4);
        assert_eq!(compiled.abilities()[0].name(), "Crossbow");
        assert_eq!(
            compiled.copy().closed_feature_literal,
            CORE_CLOSED_FEATURE_LITERAL
        );
        assert_eq!(
            compiled.copy().not_equipped_literal,
            CORE_NOT_EQUIPPED_LITERAL
        );
    }

    #[test]
    fn locale_schema_and_mandated_literals_fail_closed() {
        let mut copy = checked_in_copy();
        copy.locale = "en-GB".to_owned();
        assert!(validate_copy(&copy).is_err());

        let mut copy = checked_in_copy();
        copy.schema_version += 1;
        assert!(validate_copy(&copy).is_err());

        let mut copy = checked_in_copy();
        copy.closed_feature_literal = "COMING SOON".to_owned();
        assert!(validate_copy(&copy).is_err());

        let mut copy = checked_in_copy();
        copy.not_equipped_literal = "NONE".to_owned();
        assert!(validate_copy(&copy).is_err());
    }

    #[test]
    fn missing_extra_and_malformed_placeholders_fail_closed() {
        let mut copy = checked_in_copy();
        copy.populated_slot_template = copy.populated_slot_template.replace("{level}", "");
        assert!(validate_copy(&copy).is_err());

        let mut copy = checked_in_copy();
        copy.footer_template.push_str(" {invented}");
        assert!(validate_copy(&copy).is_err());

        let mut copy = checked_in_copy();
        copy.status_template.push('{');
        assert!(validate_copy(&copy).is_err());
    }

    #[test]
    fn empty_control_and_unknown_copy_fields_fail_closed() {
        let mut copy = checked_in_copy();
        copy.phases.disabled.clear();
        assert!(validate_copy(&copy).is_err());

        let mut copy = checked_in_copy();
        copy.retry_action.push('\t');
        assert!(validate_copy(&copy).is_err());

        let mut raw: serde_json::Value =
            crate::read_json(&content_root().join(CORE_IDENTITY_COPY_PATH)).expect("raw copy");
        raw.as_object_mut()
            .expect("copy object")
            .insert("invented_copy".to_owned(), serde_json::json!("unsafe"));
        assert!(serde_json::from_value::<CoreIdentityCopyFile>(raw).is_err());
    }
}

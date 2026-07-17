//! Fail-closed compiler for the unpromoted `GB-M03-07` successor-recovery presentation target.
//!
//! Authority: `Gravebound_Production_GDD_v1_Canonical.md` (`DTH-020`, `DTH-021`, `UI-008`,
//! `UI-009`), `Gravebound_Content_Production_Spec_v1.md` (`CONT-CATALOG-003`,
//! `CONT-HUB-001`), and `Gravebound_Development_Roadmap_v1.md` (`GB-M03-07`).

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use content_schema::{
    ContentId, CoreSuccessorRecoveryCopyFile, CoreSuccessorRecoveryDevelopmentTarget,
    CoreSuccessorRecoveryTargetKind, SCHEMA_VERSION,
};

use crate::{
    CORE_DEVELOPMENT_BASE_SPRITE_ID, CoreDevelopmentDeathView, CoreDevelopmentIdentity,
    CoreDevelopmentWorldFlow, FIRST_PLAYABLE_CLASS_ID, load_core_development_death_view,
    load_core_development_identity, load_core_development_identity_copy,
    load_core_development_world_flow,
};

pub const CORE_SUCCESSOR_RECOVERY_TARGET_NAME: &str = "core-dev-successor-recovery";
pub const CORE_SUCCESSOR_RECOVERY_TARGET_PATH: &str = "core_dev/successor_recovery.json";
pub const CORE_SUCCESSOR_RECOVERY_COPY_PATH: &str = "core_dev/successor_recovery.en-US.json";
pub const CORE_SUCCESSOR_RECOVERY_HALL_ID: &str = "hub.lantern_halls_01";

/// Closed copy keys prevent runtime presentation from inventing unreviewed recovery text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoreSuccessorRecoveryCopyKey {
    SurfaceEyebrow,
    SurfaceTitle,
    SurfaceSubtitle,
    BadgeSelected,
    FieldLevel,
    FieldOathNone,
    FieldNewStarterKit,
    FieldSafeCharacterSelect,
    FieldConfirmationOne,
    FieldConfirmationTwo,
    ActionPlay,
    ActionRetry,
    InputPlay,
    StatusCreating,
    StatusReady,
    StatusEnteringHall,
    StatusLoadingHall,
    StatusHallReady,
    StatusRecoverable,
    StatusFatal,
    StatusUpdate,
    FooterCleanRecovery,
    FooterAuthority,
}

impl CoreSuccessorRecoveryCopyKey {
    pub const ALL: [Self; 23] = [
        Self::SurfaceEyebrow,
        Self::SurfaceTitle,
        Self::SurfaceSubtitle,
        Self::BadgeSelected,
        Self::FieldLevel,
        Self::FieldOathNone,
        Self::FieldNewStarterKit,
        Self::FieldSafeCharacterSelect,
        Self::FieldConfirmationOne,
        Self::FieldConfirmationTwo,
        Self::ActionPlay,
        Self::ActionRetry,
        Self::InputPlay,
        Self::StatusCreating,
        Self::StatusReady,
        Self::StatusEnteringHall,
        Self::StatusLoadingHall,
        Self::StatusHallReady,
        Self::StatusRecoverable,
        Self::StatusFatal,
        Self::StatusUpdate,
        Self::FooterCleanRecovery,
        Self::FooterAuthority,
    ];

    #[must_use]
    pub const fn content_id(self) -> &'static str {
        match self {
            Self::SurfaceEyebrow => "successor.surface.eyebrow",
            Self::SurfaceTitle => "successor.surface.title",
            Self::SurfaceSubtitle => "successor.surface.subtitle",
            Self::BadgeSelected => "successor.badge.selected",
            Self::FieldLevel => "successor.field.level",
            Self::FieldOathNone => "successor.field.oath_none",
            Self::FieldNewStarterKit => "successor.field.new_starter_kit",
            Self::FieldSafeCharacterSelect => "successor.field.safe_character_select",
            Self::FieldConfirmationOne => "successor.field.confirmation_one",
            Self::FieldConfirmationTwo => "successor.field.confirmation_two",
            Self::ActionPlay => "successor.action.play",
            Self::ActionRetry => "successor.action.retry",
            Self::InputPlay => "successor.input.play",
            Self::StatusCreating => "successor.status.creating",
            Self::StatusReady => "successor.status.ready",
            Self::StatusEnteringHall => "successor.status.entering_hall",
            Self::StatusLoadingHall => "successor.status.loading_hall",
            Self::StatusHallReady => "successor.status.hall_ready",
            Self::StatusRecoverable => "successor.status.recoverable",
            Self::StatusFatal => "successor.status.fatal",
            Self::StatusUpdate => "successor.status.update",
            Self::FooterCleanRecovery => "successor.footer.clean_recovery",
            Self::FooterAuthority => "successor.footer.authority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSuccessorRecoveryHashes {
    pub records_blake3: String,
    pub localization_blake3: String,
}

/// Immutable, renderer-independent recovery copy and dependency authority.
#[derive(Debug, Clone)]
pub struct CoreSuccessorRecoveryContent {
    target_name: String,
    class_id: ContentId,
    class_name: String,
    hall_id: ContentId,
    hall_name: String,
    appearance_id: ContentId,
    copy: BTreeMap<String, String>,
    hashes: CoreSuccessorRecoveryHashes,
}

impl CoreSuccessorRecoveryContent {
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    #[must_use]
    pub fn class_id(&self) -> &str {
        self.class_id.as_str()
    }

    #[must_use]
    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    #[must_use]
    pub fn hall_id(&self) -> &str {
        self.hall_id.as_str()
    }

    #[must_use]
    pub fn hall_name(&self) -> &str {
        &self.hall_name
    }

    #[must_use]
    pub fn appearance_id(&self) -> &str {
        self.appearance_id.as_str()
    }

    #[must_use]
    pub const fn hashes(&self) -> &CoreSuccessorRecoveryHashes {
        &self.hashes
    }

    #[must_use]
    pub fn copy(&self, key: CoreSuccessorRecoveryCopyKey) -> &str {
        self.copy
            .get(key.content_id())
            .map(String::as_str)
            .expect("validated successor recovery copy must remain present")
    }
}

pub fn load_core_successor_recovery(root: &Path) -> Result<CoreSuccessorRecoveryContent> {
    let target: CoreSuccessorRecoveryDevelopmentTarget =
        crate::read_json(&root.join(CORE_SUCCESSOR_RECOVERY_TARGET_PATH))?;
    let copy: CoreSuccessorRecoveryCopyFile =
        crate::read_json(&root.join(CORE_SUCCESSOR_RECOVERY_COPY_PATH))?;
    compile_core_successor_recovery(root, &target, &copy)
}

pub fn compile_core_successor_recovery(
    root: &Path,
    target: &CoreSuccessorRecoveryDevelopmentTarget,
    copy: &CoreSuccessorRecoveryCopyFile,
) -> Result<CoreSuccessorRecoveryContent> {
    validate_target(target)?;
    let identity = load_core_development_identity(root)
        .context("successor recovery identity dependency failed validation")?;
    let identity_copy = load_core_development_identity_copy(root)
        .context("successor recovery identity copy dependency failed validation")?;
    let world = load_core_development_world_flow(root)
        .context("successor recovery world-flow dependency failed validation")?;
    let death = load_core_development_death_view(root)
        .context("successor recovery death-view dependency failed validation")?;
    validate_dependencies(target, &identity, &world)?;
    let localization = validate_copy(target, copy)?;
    let records_blake3 = dependency_hash(target, &identity, &world, &death);
    let localization_blake3 = hash_file(&root.join(CORE_SUCCESSOR_RECOVERY_COPY_PATH))?;
    if records_blake3 != target.expected_records_blake3 {
        bail!("Core successor recovery records hash mismatch: {records_blake3}");
    }
    if localization_blake3 != target.expected_localization_blake3 {
        bail!("Core successor recovery localization hash mismatch: {localization_blake3}");
    }
    let hall_name = world
        .localized("hub.lantern_halls_01.name")
        .context("successor recovery Hall name is missing from world authority")?
        .to_owned();
    Ok(CoreSuccessorRecoveryContent {
        target_name: target.target_name.clone(),
        class_id: target.required_class_id.clone(),
        class_name: identity_copy.class_name().to_owned(),
        hall_id: target.required_hall_id.clone(),
        hall_name,
        appearance_id: target.required_appearance_id.clone(),
        copy: localization,
        hashes: CoreSuccessorRecoveryHashes {
            records_blake3,
            localization_blake3,
        },
    })
}

fn validate_target(target: &CoreSuccessorRecoveryDevelopmentTarget) -> Result<()> {
    if target.schema_version != SCHEMA_VERSION {
        bail!("Core successor recovery schema version must be {SCHEMA_VERSION}");
    }
    if target.target_kind != CoreSuccessorRecoveryTargetKind::UnpromotedSuccessorRecoverySubset {
        bail!("Core successor recovery target kind is invalid");
    }
    if target.target_name != CORE_SUCCESSOR_RECOVERY_TARGET_NAME {
        bail!("Core successor recovery target name is invalid");
    }
    if target.required_class_id.as_str() != FIRST_PLAYABLE_CLASS_ID
        || target.required_hall_id.as_str() != CORE_SUCCESSOR_RECOVERY_HALL_ID
        || target.required_appearance_id.as_str() != CORE_DEVELOPMENT_BASE_SPRITE_ID
    {
        bail!("Core successor recovery dependency identity is invalid");
    }
    let required = CoreSuccessorRecoveryCopyKey::ALL
        .map(CoreSuccessorRecoveryCopyKey::content_id)
        .into_iter()
        .collect::<Vec<_>>();
    let actual = target
        .required_copy_ids
        .iter()
        .map(ContentId::as_str)
        .collect::<Vec<_>>();
    if actual != required {
        bail!("Core successor recovery copy allowlist is incomplete or reordered");
    }
    validate_hash(&target.expected_records_blake3, "records")?;
    validate_hash(&target.expected_localization_blake3, "localization")?;
    Ok(())
}

fn validate_dependencies(
    target: &CoreSuccessorRecoveryDevelopmentTarget,
    identity: &CoreDevelopmentIdentity,
    world: &CoreDevelopmentWorldFlow,
) -> Result<()> {
    if identity.class().header.id != target.required_class_id
        || identity.base_sprite_id() != &target.required_appearance_id
    {
        bail!("Core successor recovery identity dependency drifted");
    }
    if world.hub().header.id != target.required_hall_id {
        bail!("Core successor recovery Hall dependency drifted");
    }
    Ok(())
}

fn validate_copy(
    target: &CoreSuccessorRecoveryDevelopmentTarget,
    copy: &CoreSuccessorRecoveryCopyFile,
) -> Result<BTreeMap<String, String>> {
    if copy.schema_version != SCHEMA_VERSION || copy.locale != "en-US" {
        bail!("Core successor recovery copy identity is invalid");
    }
    if copy.entries.len() != target.required_copy_ids.len() {
        bail!("Core successor recovery copy count is invalid");
    }
    let mut localized = BTreeMap::new();
    for (expected, entry) in target.required_copy_ids.iter().zip(&copy.entries) {
        if &entry.key != expected {
            bail!("Core successor recovery copy order or identity is invalid");
        }
        validate_copy_value(entry.key.as_str(), &entry.value)?;
        if localized
            .insert(entry.key.as_str().to_owned(), entry.value.clone())
            .is_some()
        {
            bail!("Core successor recovery copy contains a duplicate key");
        }
    }
    let level = localized
        .get(CoreSuccessorRecoveryCopyKey::FieldLevel.content_id())
        .context("successor level copy is missing")?;
    let level_without_placeholder = level.replace("{level}", "");
    if level.matches("{level}").count() != 1
        || level_without_placeholder.contains('{')
        || level_without_placeholder.contains('}')
    {
        bail!("Core successor recovery level copy has invalid placeholders");
    }
    if localized
        .iter()
        .filter(|(key, _)| key.as_str() != CoreSuccessorRecoveryCopyKey::FieldLevel.content_id())
        .any(|(_, value)| value.contains('{') || value.contains('}'))
    {
        bail!("Core successor recovery copy has an unauthorized placeholder");
    }
    Ok(localized)
}

fn validate_copy_value(key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 512 {
        bail!("Core successor recovery copy {key} is empty or oversized");
    }
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n')
    {
        bail!("Core successor recovery copy {key} contains a control character");
    }
    Ok(())
}

fn dependency_hash(
    target: &CoreSuccessorRecoveryDevelopmentTarget,
    identity: &CoreDevelopmentIdentity,
    world: &CoreDevelopmentWorldFlow,
    death: &CoreDevelopmentDeathView,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"gravebound.core-successor-recovery.dependencies.v1\0");
    for value in [
        target.required_class_id.as_str(),
        target.required_hall_id.as_str(),
        target.required_appearance_id.as_str(),
        identity.source_content_version(),
        world.target_name(),
        &world.hashes().records_blake3,
        &world.hashes().assets_blake3,
        &world.hashes().localization_blake3,
        death.target_name(),
        &death.hashes().records_blake3,
        &death.hashes().assets_blake3,
        &death.hashes().localization_blake3,
        death.item_content_revision(),
    ] {
        let bytes = value.as_bytes();
        hasher.update(&u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(bytes);
    }
    hasher.finalize().to_hex().to_string()
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn validate_hash(value: &str, label: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("Core successor recovery {label} hash must be 64 hexadecimal characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content")
    }

    fn raw() -> (
        CoreSuccessorRecoveryDevelopmentTarget,
        CoreSuccessorRecoveryCopyFile,
    ) {
        let root = content_root();
        (
            crate::read_json(&root.join(CORE_SUCCESSOR_RECOVERY_TARGET_PATH)).unwrap(),
            crate::read_json(&root.join(CORE_SUCCESSOR_RECOVERY_COPY_PATH)).unwrap(),
        )
    }

    #[test]
    fn checked_in_recovery_target_is_transitive_closed_and_localized() {
        let compiled = load_core_successor_recovery(&content_root()).unwrap();
        assert_eq!(compiled.target_name(), CORE_SUCCESSOR_RECOVERY_TARGET_NAME);
        assert_eq!(compiled.class_id(), FIRST_PLAYABLE_CLASS_ID);
        assert_eq!(compiled.hall_id(), CORE_SUCCESSOR_RECOVERY_HALL_ID);
        assert_eq!(compiled.appearance_id(), CORE_DEVELOPMENT_BASE_SPRITE_ID);
        assert_eq!(compiled.class_name(), "Grave Arbalist");
        assert_eq!(compiled.hall_name(), "Lantern Halls");
        assert_eq!(
            compiled.copy(CoreSuccessorRecoveryCopyKey::ActionPlay),
            "PLAY"
        );
    }

    #[test]
    fn copy_allowlist_order_unknown_fields_and_placeholders_fail_closed() {
        let (target, mut copy) = raw();
        copy.entries.swap(0, 1);
        assert!(compile_core_successor_recovery(&content_root(), &target, &copy).is_err());

        let (_, mut copy) = raw();
        copy.entries
            .iter_mut()
            .find(|entry| {
                entry.key.as_str() == CoreSuccessorRecoveryCopyKey::FieldLevel.content_id()
            })
            .unwrap()
            .value = "LEVEL {rank}".to_owned();
        assert!(compile_core_successor_recovery(&content_root(), &target, &copy).is_err());

        let target_text =
            fs::read_to_string(content_root().join(CORE_SUCCESSOR_RECOVERY_TARGET_PATH)).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&target_text).unwrap();
        value["release_stage"] = serde_json::json!("core");
        assert!(serde_json::from_value::<CoreSuccessorRecoveryDevelopmentTarget>(value).is_err());
    }
}

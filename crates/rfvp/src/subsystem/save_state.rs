#[cfg(feature = "no_std")]
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use anyhow::{bail, Context, Result};
#[cfg(not(feature = "no_std"))]
use bincode::Options;
use serde::{Deserialize, Serialize};

use crate::audio_player::{BgmPlayerSnapshotV1, SePlayerSnapshotV1};
use crate::script::global::GLOBAL;
use crate::script::Variant;
use crate::subsystem::resources::motion_manager::MotionManagerSnapshotV1;
use crate::subsystem::resources::thread_manager::{ThreadManager, ThreadManagerSnapshotV1};
use crate::subsystem::world::GameData;

const SAVE_STATE_MAGIC: [u8; 4] = *b"RFVS";
const SAVE_STATE_FOOTER_LEN: usize = 8; // u32 payload_len + 4-byte magic
const MAX_STATE_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
/// Plugin-profile metadata block placed immediately before the V1 state payload:
/// `metadata JSON | u32 metadata_len | RFPM | bincode V1 state | u32 state_len | RFVS`.
const PROFILE_MAGIC: [u8; 4] = *b"RFPM";
const PROFILE_FOOTER_LEN: usize = 8; // u32 metadata_len + 4-byte magic
pub const MAX_PROFILE_METADATA_BYTES: usize = 64 * 1024;

#[cfg(not(feature = "no_std"))]
fn bincode_opts() -> impl bincode::Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .reject_trailing_bytes()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSnapshotV1 {
    pub bgm: BgmPlayerSnapshotV1,
    pub se: SePlayerSnapshotV1,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveStateSnapshotV1 {
    pub version: u16,
    pub motion: MotionManagerSnapshotV1,
    pub audio: AudioSnapshotV1,
    /// Reverse-engineered slot save persists non-volatile globals together with the scene snapshot.
    pub globals_non_volatile: Vec<Variant>,
    /// Full VM coroutine state needed for resumable saves.
    pub vm: ThreadManagerSnapshotV1,
}

impl SaveStateSnapshotV1 {
    pub fn capture(game_data: &mut GameData) -> Self {
        let globals_non_volatile = GLOBAL.lock().unwrap().snapshot_non_volatile();

        let vm = match game_data.save_manager.take_pending_vm_snapshot() {
            Some(s) => s,
            None => {
                log::warn!("SaveStateSnapshotV1: missing VM snapshot; save will not be resumable");
                ThreadManager::new().capture_snapshot_v1()
            }
        };

        SaveStateSnapshotV1 {
            version: 1,
            motion: game_data.motion_manager.capture_snapshot_v1(),
            audio: AudioSnapshotV1 {
                bgm: game_data.bgm_player_ref().capture_snapshot_v1(),
                se: game_data.se_player_ref().capture_snapshot_v1(),
            },
            globals_non_volatile,
            vm,
        }
    }

    pub fn apply(&self, game_data: &mut GameData, tm: &mut ThreadManager) -> Result<()> {
        let GameData {
            motion_manager,
            vfs,
            bgm_player,
            se_player,
            ..
        } = game_data;

        motion_manager
            .apply_snapshot_v1(&self.motion, vfs)
            .context("apply MotionManagerSnapshotV1")?;

        bgm_player
            .apply_snapshot_v1(&self.audio.bgm, vfs)
            .context("apply BgmPlayerSnapshotV1")?;

        se_player
            .apply_snapshot_v1(&self.audio.se, vfs)
            .context("apply SePlayerSnapshotV1")?;

        {
            let mut g = GLOBAL.lock().unwrap();
            g.restore_non_volatile(&self.globals_non_volatile);
        }

        tm.apply_snapshot_v1(&self.vm);
        Ok(())
    }
}

/// A resumable save state together with its session profile metadata.
///
/// `profile_metadata` is `None` only for hosts without a session identity; those
/// hosts keep writing the historical layout.
#[derive(Debug, Clone)]
pub struct ResumableSaveState {
    pub snapshot: SaveStateSnapshotV1,
    pub profile_metadata: Option<Vec<u8>>,
}

#[cfg(not(feature = "no_std"))]
impl ResumableSaveState {
    /// Encode the profile first so a failure does not consume the pending VM snapshot.
    pub fn capture(game_data: &mut GameData) -> Result<Self> {
        let profile_metadata = session_profile_metadata(game_data)?;
        Ok(Self {
            snapshot: SaveStateSnapshotV1::capture(game_data),
            profile_metadata,
        })
    }
}

#[cfg(all(
    feature = "desktop-preparation",
    not(feature = "no_std"),
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
fn session_profile_metadata(game_data: &GameData) -> Result<Option<Vec<u8>>> {
    game_data
        .plugin_session
        .as_ref()
        .map(crate::preparation::profile::encode_metadata)
        .transpose()
}

#[cfg(all(
    not(feature = "no_std"),
    not(all(
        feature = "desktop-preparation",
        any(target_os = "macos", target_os = "windows", target_os = "linux")
    ))
))]
fn session_profile_metadata(_game_data: &GameData) -> Result<Option<Vec<u8>>> {
    Ok(None)
}

/// Append `[profile block] | V1 state | RFVS footer`.
pub fn append_resumable_state(out: &mut Vec<u8>, state: &ResumableSaveState) -> Result<()> {
    if let Some(metadata) = &state.profile_metadata {
        append_profile_block(out, metadata)?;
    }
    append_state_chunk_v1(out, &state.snapshot)
}

fn append_profile_block(out: &mut Vec<u8>, metadata: &[u8]) -> Result<()> {
    if metadata.len() > MAX_PROFILE_METADATA_BYTES {
        bail!(
            "save profile metadata too large: {} bytes (max {})",
            metadata.len(),
            MAX_PROFILE_METADATA_BYTES
        );
    }
    out.extend_from_slice(metadata);
    out.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
    out.extend_from_slice(&PROFILE_MAGIC);
    Ok(())
}

pub fn append_state_chunk_v1(out: &mut Vec<u8>, snap: &SaveStateSnapshotV1) -> Result<()> {
    #[cfg(feature = "no_std")]
    {
        let _ = (out, snap);
        bail!("SaveStateSnapshotV1 serialization requires the std desktop bincode adapter");
    }

    #[cfg(not(feature = "no_std"))]
    {
        let payload = bincode_opts()
            .serialize(snap)
            .context("serialize SaveStateSnapshotV1")?;

        if payload.len() > MAX_STATE_PAYLOAD_BYTES {
            bail!(
                "SaveStateSnapshotV1 payload too large: {} bytes (max {})",
                payload.len(),
                MAX_STATE_PAYLOAD_BYTES
            );
        }

        let len_u32: u32 = payload
            .len()
            .try_into()
            .context("payload length overflow")?;
        out.extend_from_slice(&payload);
        out.extend_from_slice(&len_u32.to_le_bytes());
        out.extend_from_slice(&SAVE_STATE_MAGIC);
        Ok(())
    }
}

fn read_u32_le(bytes: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
}

#[cfg_attr(feature = "no_std", allow(dead_code))]
/// Locate the V1 payload from the trailing `RFVS` footer, exactly as historical readers do.
fn locate_state_payload(file_bytes: &[u8]) -> Result<Option<core::ops::Range<usize>>> {
    if file_bytes.len() < SAVE_STATE_FOOTER_LEN {
        return Ok(None);
    }

    let magic_pos = file_bytes.len() - 4;
    if file_bytes[magic_pos..] != SAVE_STATE_MAGIC {
        return Ok(None);
    }

    let len_pos = file_bytes.len() - SAVE_STATE_FOOTER_LEN;
    let payload_len = read_u32_le(file_bytes, len_pos) as usize;

    if payload_len > MAX_STATE_PAYLOAD_BYTES {
        bail!(
            "SaveState footer present but payload length {} exceeds max {}",
            payload_len,
            MAX_STATE_PAYLOAD_BYTES
        );
    }

    if file_bytes.len() < SAVE_STATE_FOOTER_LEN + payload_len {
        bail!(
            "SaveState footer present but payload is truncated: file={} need={} payload_len={} footer={}",
            file_bytes.len(),
            SAVE_STATE_FOOTER_LEN + payload_len,
            payload_len,
            SAVE_STATE_FOOTER_LEN
        );
    }

    let payload_start = file_bytes.len() - SAVE_STATE_FOOTER_LEN - payload_len;
    Ok(Some(payload_start..payload_start + payload_len))
}

/// Probe the eight bytes preceding the V1 payload for an `RFPM` block.
/// A missing marker means unknown identity; a present marker requires a complete block.
pub fn profile_metadata_before(file_bytes: &[u8], payload_start: usize) -> Result<Option<&[u8]>> {
    if payload_start < PROFILE_FOOTER_LEN
        || file_bytes[payload_start - 4..payload_start] != PROFILE_MAGIC
    {
        return Ok(None);
    }
    let len_pos = payload_start - PROFILE_FOOTER_LEN;
    let metadata_len = read_u32_le(file_bytes, len_pos) as usize;
    if metadata_len > MAX_PROFILE_METADATA_BYTES {
        bail!(
            "save profile metadata length {} exceeds max {}",
            metadata_len,
            MAX_PROFILE_METADATA_BYTES
        );
    }
    if metadata_len > len_pos {
        bail!(
            "save profile metadata is truncated: need {} bytes before offset {}",
            metadata_len,
            len_pos
        );
    }
    Ok(Some(&file_bytes[len_pos - metadata_len..len_pos]))
}

#[cfg(not(feature = "no_std"))]
fn decode_state_payload(payload: &[u8]) -> Result<SaveStateSnapshotV1> {
    let snap: SaveStateSnapshotV1 = bincode_opts()
        .deserialize(payload)
        .context("deserialize SaveStateSnapshotV1")?;
    if snap.version != 1 {
        bail!("unsupported SaveStateSnapshotV1 version: {}", snap.version);
    }
    Ok(snap)
}

/// Historical V1 suffix decoder. It deliberately ignores any profile block.
pub fn try_decode_state_chunk_v1(file_bytes: &[u8]) -> Result<Option<SaveStateSnapshotV1>> {
    #[cfg(feature = "no_std")]
    {
        let _ = file_bytes;
        return Ok(None);
    }

    #[cfg(not(feature = "no_std"))]
    {
        let Some(range) = locate_state_payload(file_bytes)? else {
            return Ok(None);
        };
        decode_state_payload(&file_bytes[range]).map(Some)
    }
}

/// Fully decoded and profile-checked load, produced before any state is mutated.
#[cfg(not(feature = "no_std"))]
#[derive(Debug)]
pub struct LoadPreflight {
    /// `None` for header-only saves (original engine or pre-RFVS saves).
    pub snapshot: Option<SaveStateSnapshotV1>,
    /// Non-fatal diagnostic, e.g. an unknown profile on an older resumable save.
    pub diagnostic: Option<String>,
}

/// Decode the state and check plugin-profile compatibility without touching `game_data`.
/// Any error rejects the whole load; callers must not fall back to a legacy restore.
#[cfg(not(feature = "no_std"))]
pub fn preflight_load(file_bytes: &[u8], game_data: &GameData) -> Result<LoadPreflight> {
    let Some(range) = locate_state_payload(file_bytes)? else {
        return Ok(LoadPreflight {
            snapshot: None,
            diagnostic: None,
        });
    };
    let metadata = profile_metadata_before(file_bytes, range.start)?;
    let snapshot = decode_state_payload(&file_bytes[range])?;
    let diagnostic = check_session_profile(metadata, game_data)?;
    Ok(LoadPreflight {
        snapshot: Some(snapshot),
        diagnostic,
    })
}

#[cfg(all(
    feature = "desktop-preparation",
    not(feature = "no_std"),
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
fn check_session_profile(metadata: Option<&[u8]>, game_data: &GameData) -> Result<Option<String>> {
    crate::preparation::profile::check_save(metadata, game_data.plugin_session.as_ref())
}

/// Hosts that cannot activate plugins behave like older RFVP releases.
#[cfg(all(
    not(feature = "no_std"),
    not(all(
        feature = "desktop-preparation",
        any(target_os = "macos", target_os = "windows", target_os = "linux")
    ))
))]
fn check_session_profile(
    metadata: Option<&[u8]>,
    _game_data: &GameData,
) -> Result<Option<String>> {
    Ok(metadata.map(|_| "save plugin profile is not enforced by this host build".into()))
}

#[cfg(all(test, not(feature = "no_std")))]
#[path = "save_state_tests.rs"]
mod tests;

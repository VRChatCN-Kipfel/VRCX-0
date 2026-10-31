pub mod auth;
pub mod avatars;
pub mod collections;
pub mod discovery;
pub mod favorites;
pub mod game;
pub mod media;
pub mod profile;
pub mod remote;
pub mod social;
pub mod telemetry;
pub mod vrcc;

mod event_payloads;
mod scope_gate;

pub(crate) fn wire_count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

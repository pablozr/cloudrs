//! OS integration for cloudrs (ADR 0010): keychain, sign-in window, Discord,
//! the media controls (ADR 0019) and in-app updates (ADR 0026). It knows
//! nothing of SoundCloud's API or the core: the app wires it in at the
//! composition root.

pub mod discord;
pub mod keychain;
pub mod media;
pub mod sign_in;
pub mod update;

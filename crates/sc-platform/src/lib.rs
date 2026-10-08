//! OS integration for cloudrs (ADR 0010): keychain, sign-in window, Discord
//! and the media controls (ADR 0019). It knows nothing of SoundCloud's API
//! or the core: the app wires it in at the composition root.

pub mod discord;
pub mod keychain;
pub mod media;
pub mod sign_in;

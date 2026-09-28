//! Plugin ecosystem surface.
//!
//! The built-in plugin/theme stack is retired (niubash#145, owner decision
//! 2026-09-28): the host no longer ships a pack inventory, an oh-my-niu
//! bundle loader, native plugin presets, or built-in themes. NIU_PLUGINS,
//! NIU_THEME, NIU_THEME_PLUGIN and NIU_DISABLE_DEFAULT_PLUGINS lines in an
//! existing rc stay legal shell assignments - nothing reads them any more.
//!
//! What remains is the external plugin-manager *source* protocol
//! (`niu plugin source ...`): installing, reviewing, trusting and surfacing
//! third-party plugin managers such as oh-my-bash as first-class origins.

pub mod sources;

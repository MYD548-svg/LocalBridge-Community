//! Browser transport is an authenticated local client, not a new runtime mode.
pub mod authority;
pub mod host;
pub mod package;
pub mod protocol;
pub mod service;

pub const PROTOCOL_VERSION: u32 = 1;
#[cfg(not(feature = "browser-host-dev"))]
pub const HOST_NAME: &str = "com.localbridge.chatgpt_web";
#[cfg(feature = "browser-host-dev")]
pub const HOST_NAME: &str = "com.localbridge.chatgpt_web_dev";
#[cfg(not(feature = "browser-host-dev"))]
pub const EXTENSION_ID: &str = include_str!("../../../extensions/chatgpt-web/extension-id.txt");
#[cfg(feature = "browser-host-dev")]
pub const EXTENSION_ID: &str = include_str!("../../../extensions/chatgpt-web/extension-id.dev.txt");
#[cfg(not(feature = "browser-host-dev"))]
pub const IDENTITY: &str = include_str!("../../../extensions/chatgpt-web/identity.json");
#[cfg(feature = "browser-host-dev")]
pub const IDENTITY: &str = include_str!("../../../extensions/chatgpt-web/identity.dev.json");

pub fn allowed_origin(origin: &str) -> bool {
    origin == format!("chrome-extension://{}/", EXTENSION_ID.trim())
}

pub fn control_pipe_name(install_id: &str) -> std::io::Result<String> {
    crate::local_connection::pipe::pipe_name(install_id)
        .map(|name| name.replace("LocalBridge-MCP-", "LocalBridge-Browser-Control-"))
}

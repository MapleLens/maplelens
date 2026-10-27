//! MapleLens core: everything that does not touch the operating system or the browser.
//!
//! Frames come in as RGB pixels and cards go out as RGBA pixels, so the Windows app and the web app share
//! all detection, reading, market logic and drawing; each only supplies screen capture, the mouse and a place
//! to show the card.

pub mod config;
pub mod geometry;
pub mod hotkey;
pub mod image;
pub mod mapinfo;
pub mod market;
pub mod notify;
pub mod pipeline;
pub mod placement;
pub mod ranking;
pub mod render;
pub mod scale_vote;
pub mod settings;
pub mod shop;
pub mod vision;

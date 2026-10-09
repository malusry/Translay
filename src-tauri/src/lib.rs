mod app;
mod capture_coordinator;
mod capture_session;
mod clipboard_service;
mod config;
mod credential_store;
mod explanation;
mod foreground_context;
mod hotkey_service;
mod latest_capture_store;
mod model_config;
mod models;
mod overlay_manager;
mod overlay_policy;
mod reasoning;
mod selection_button;
mod selection_placement;
mod selection_retention;
mod selection_service;
mod single_instance;
mod tone_translation;
pub mod translation;
mod translation_cache;
mod translation_service;
mod translation_task;

pub use app::run;

#[cfg(debug_assertions)]
mod document_probe;

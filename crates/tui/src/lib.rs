//! # filegoblin-tui
//!
//! An interactive terminal file-explorer and preview interface for Filegoblin.
//! This crate provides the `run_tui` entry point which renders a full-screen
//! TUI powered by `ratatui` and `crossterm`, allowing users to browse local
//! directories (and optionally web links) and select files for ingestion.

pub mod state;
pub mod ui;

pub use ui::run_tui;

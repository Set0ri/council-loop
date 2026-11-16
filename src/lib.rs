//! The Council Loop: a resumable, blind, checked decision process for AI agents.
//!
//! ELI5: the AI does the thinking; this program does the bookkeeping that should never be left to an
//! AI's good intentions: remembering where the run is, hiding who wrote what, shuffling, counting
//! votes, cutting 7 -> 5 -> 3 -> 1, checking the rules were followed, and locking test files.
//!
//! One binary, `council`, is every surface: the command line, the MCP server (`council mcp`) and the
//! Claude Code hook that guards locked tests (`council hook`). They all call the functions in this
//! crate, so the rules are enforced the same way everywhere.

pub mod api;
pub mod assets;
pub mod cli;
pub mod clock;
pub mod codemap;
pub mod core;
pub mod demo;
pub mod gate;
pub mod hook;
pub mod log;
pub mod mcp;
pub mod memory;
pub mod out;
pub mod providers;
pub mod py;
pub mod pyrand;
pub mod pyread;
pub mod refresh;
pub mod run;
pub mod safe;
pub mod tpl;
pub mod val;
pub mod welcome;
pub mod yaml;

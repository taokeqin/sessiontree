//! Native explorer for the AI coding sessions already kept on this machine.
mod capture;
mod model;
mod scan;
mod share;
mod state;
mod ui;

use nsh_core::prelude::*;
pub use ui::App;

#[native_module("sessiontree/native")]
fn native(_registry: &mut Registry) {}

pub fn scan_report() -> Result<String, String> {
    scan::report()
}

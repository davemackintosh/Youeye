//! Design tokens and variables.
//!
//! On disk these live as CSS custom properties inside the SVG `<style>` root:
//!
//! ```css
//! :root {
//!     --token-brand-primary: #0052cc;
//!     --var-rhythm: 8px;
//!     --var-padding-default: calc(2 * var(--var-rhythm));
//! }
//! ```
//!
//! In memory the leading `--token-` / `--var-` prefix is stripped — keys are
//! bare names (`"brand-primary"`, `"rhythm"`) and values are kept as raw CSS
//! strings. Expression evaluation is a later phase's problem.
//!
//! Parsing from the style block lives in `youeye-io`, not here. This module
//! is just the typed dictionaries the editor reads.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tokens(pub BTreeMap<String, String>);

impl Tokens {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.insert(name.into(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Variables(pub BTreeMap<String, String>);

impl Variables {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.insert(name.into(), value.into());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

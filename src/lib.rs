mod consts;
mod highlight;
mod semantic_tokens_builder;

use crate::{
    consts::{SEMANTIC_TOKEN_MODIFIERS, SEMANTIC_TOKEN_TYPES},
    highlight::Highlight,
};
use js_sys::Uint32Array;
use lsp_types::{Position, Range, SemanticTokensDelta};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};
use tree_sitter::Language;
use tree_sitter_highlight::HighlightConfiguration;
use wasm_bindgen::prelude::*;

#[derive(Default)]
struct ViewState {
    version: u32,
    tokens: Vec<u32>,
}

struct AppState {
    highlight: Highlight,
    views: HashMap<String, ViewState>,
}

thread_local! {
    static STATE: RefCell<AppState> = RefCell::new(AppState {
        highlight: setup(),
        views: HashMap::new(),
    });
}

fn setup() -> Highlight {
    let mut highlight_config = HighlightConfiguration::new(
        Language::new(tree_sitter_rshtml::LANGUAGE),
        "rshtml",
        include_str!("../queries/rshtml/highlights.scm"),
        include_str!("../queries/rshtml/injections.scm"),
        "",
    )
    .unwrap();

    // Rust highlights
    let mut highlight_config_rust = HighlightConfiguration::new(
        Language::new(tree_sitter_rust::LANGUAGE),
        "rust",
        include_str!("../queries/rust/highlights.scm"),
        include_str!("../queries/rust/injections.scm"),
        "",
    )
    .unwrap();

    let mut cn: HashSet<String> = highlight_config
        .names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let cnr: HashSet<String> = highlight_config_rust
        .names()
        .iter()
        .map(|s| s.to_string())
        .collect();

    cn.extend(cnr);
    let final_capture_names: Vec<String> = cn.into_iter().collect();

    highlight_config.configure(final_capture_names.as_ref());
    highlight_config_rust.configure(final_capture_names.as_ref());

    let mut highlights = Highlight::new(highlight_config, final_capture_names);
    highlights
        .highlight_injects
        .insert("rust", highlight_config_rust);

    highlights
}

#[wasm_bindgen]
pub fn semantic_token_types() -> Result<JsValue, JsError> {
    Ok(serde_wasm_bindgen::to_value(&SEMANTIC_TOKEN_TYPES)?)
}

#[wasm_bindgen]
pub fn semantic_token_modifiers() -> Result<JsValue, JsError> {
    Ok(serde_wasm_bindgen::to_value(&SEMANTIC_TOKEN_MODIFIERS)?)
}

#[wasm_bindgen]
pub fn semantic_tokens_full(uri: &str, source: &str) -> Result<Uint32Array, JsError> {
    STATE.with_borrow_mut(|state| {
        let tokens = state.highlight.highlight(source, None)?;

        let view = state.views.entry(uri.to_string()).or_default();
        view.version += 1;

        let js_array = Uint32Array::from(tokens.as_slice());

        view.tokens = tokens;

        Ok(js_array)
    })
}

#[wasm_bindgen]
pub fn semantic_tokens_range(source: &str, range: JsValue) -> Result<Uint32Array, JsError> {
    STATE.with_borrow_mut(|state| {
        let range: Range = serde_wasm_bindgen::from_value(range)?;

        let start_byte = position_to_byte_offset(source, range.start);
        let end_byte = position_to_byte_offset(source, range.end);
        let tokens = state
            .highlight
            .highlight(source, Some(start_byte..end_byte))?;

        Ok(Uint32Array::from(tokens.as_slice()))
    })
}

#[wasm_bindgen]
pub fn semantic_tokens_full_delta(uri: &str, source: &str) -> Result<JsValue, JsError> {
    STATE.with_borrow_mut(|state| {
        let tokens = state.highlight.highlight(source, None)?;

        let edits = if let Some(view) = state.views.get(uri) {
            state
                .highlight
                .semantic_tokens_difference(&view.tokens, &tokens)
        } else {
            state.highlight.semantic_tokens_difference(&[], &tokens)
        };

        let view = state.views.entry(uri.to_string()).or_default();
        view.version += 1;
        view.tokens = tokens;

        let res = SemanticTokensDelta {
            result_id: Some(view.version.to_string()),
            edits,
        };

        Ok(serde_wasm_bindgen::to_value(&res)?)
    })
}

#[wasm_bindgen]
pub fn remove_document(uri: &str) {
    STATE.with_borrow_mut(|state| {
        state.views.remove(uri);
    })
}

fn position_to_byte_offset(source: &str, pos: Position) -> usize {
    let mut current_line = 0;
    let mut byte_offset = 0;

    for line in source.split_inclusive('\n') {
        if current_line == pos.line {
            let mut utf16_col = 0;
            for (ch_byte_idx, ch) in line.char_indices() {
                if utf16_col >= pos.character as usize {
                    return byte_offset + ch_byte_idx;
                }
                utf16_col += ch.len_utf16();
            }
            return byte_offset + line.len();
        }
        byte_offset += line.len();
        current_line += 1;
    }

    source.len()
}

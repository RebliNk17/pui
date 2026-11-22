//! Edit Model dialog (single and multi-edit) for Settings → Models.
//! Persists model capability/limit/cost/compat overrides to Pi's `models.json`
//! (`modelOverrides`), per-model default thinking level to `settings.json`
//! (`modelThinkingLevels`), and automatic fallback targets to `pui-model-fallbacks.json`.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub(in crate::app) const FALLBACKS_FILE: &str = "pui-model-fallbacks.json";

pub(in crate::app) const THINKING_LEVEL_OPTIONS: [(&str, &str); 8] = [
    ("", "Use global default"),
    ("off", "Off"),
    ("minimal", "Minimal"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra High"),
    ("max", "Max"),
];

pub(in crate::app) const THINKING_FORMAT_OPTIONS: [(&str, &str); 9] = [
    ("", "Auto (provider default)"),
    ("openai", "OpenAI"),
    ("deepseek", "DeepSeek"),
    ("qwen", "Qwen"),
    ("zai", "Z.AI"),
    ("openrouter", "OpenRouter"),
    ("together", "Together"),
    ("chat-template", "Chat template"),
    ("qwen-chat-template", "Qwen chat template"),
];

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) struct ModelOverride {
    pub name: Option<String>,
    pub vision: Option<bool>,
    pub reasoning: Option<bool>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub input_cost: Option<f64>,
    pub output_cost: Option<f64>,
    pub cache_read_cost: Option<f64>,
    pub cache_write_cost: Option<f64>,
    pub thinking_format: Option<String>,
}

impl ModelOverride {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.vision.is_none()
            && self.reasoning.is_none()
            && self.context_window.is_none()
            && self.max_tokens.is_none()
            && self.input_cost.is_none()
            && self.output_cost.is_none()
            && self.cache_read_cost.is_none()
            && self.cache_write_cost.is_none()
            && self.thinking_format.is_none()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum TriToggle {
    #[default]
    Keep,
    On,
    Off,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) enum FieldEdit<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::app) struct ModelEditTarget {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub vision: bool,
    pub reasoning: bool,
    pub context_window: u64,
    pub max_tokens: u64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_write_cost: f64,
    pub thinking_format: Option<String>,
    pub thinking_level: Option<String>,
    pub fallback: Option<String>,
    pub has_customizations: bool,
}

impl ModelEditTarget {
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider, self.id)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) struct ModelEditPatch {
    pub targets: Vec<(String, String)>,
    pub name: FieldEdit<String>,
    pub vision: FieldEdit<bool>,
    pub reasoning: FieldEdit<bool>,
    pub context_window: FieldEdit<u64>,
    pub max_tokens: FieldEdit<u64>,
    pub input_cost: FieldEdit<f64>,
    pub output_cost: FieldEdit<f64>,
    pub cache_read_cost: FieldEdit<f64>,
    pub cache_write_cost: FieldEdit<f64>,
    pub thinking_level: FieldEdit<String>,
    pub thinking_format: FieldEdit<String>,
    pub fallback: FieldEdit<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MultiFallbackMode {
    Keep,
    Set,
    Clear,
}

pub(in crate::app) struct ModelEditDraft {
    pub targets: Vec<ModelEditTarget>,
    // Single-edit initial snapshot for dirty tracking (avoids overriding untouched fields).
    initial: Option<ModelEditTarget>,
    initial_context_text: String,
    initial_max_tokens_text: String,
    // Form state
    pub name: String,
    pub vision: bool,
    pub reasoning: bool,
    pub vision_tri: TriToggle,
    pub reasoning_tri: TriToggle,
    pub context_text: String,
    pub max_tokens_text: String,
    pub input_cost_text: String,
    pub output_cost_text: String,
    pub cache_read_cost_text: String,
    pub cache_write_cost_text: String,
    pub fallback: Option<String>,
    multi_fallback_mode: MultiFallbackMode,
    /// `None` in multi-edit means "Keep existing"; `Some("")` means global default / auto.
    pub thinking_level: Option<String>,
    pub thinking_format: Option<String>,
    pub advanced_open: bool,
    pub fallback_open: bool,
    pub fallback_list: crate::app::model_picker::ListState,
    pub error: Option<String>,
}

pub(in crate::app) enum EditorOutcome {
    Open,
    Cancel,
    Save(ModelEditPatch),
    Reset(Vec<(String, String)>),
}

impl ModelEditDraft {
    pub fn single(target: ModelEditTarget) -> Self {
        let context_text = format_tokens_exact(target.context_window.max(4_096));
        let max_tokens_text = format_tokens_exact(target.max_tokens.max(1_024));
        let input_cost_text = format_price(target.input_cost);
        let output_cost_text = format_price(target.output_cost);
        let cache_read_cost_text = format_price(target.cache_read_cost);
        let cache_write_cost_text = format_price(target.cache_write_cost);
        let thinking_level = Some(target.thinking_level.clone().unwrap_or_default());
        let thinking_format = Some(target.thinking_format.clone().unwrap_or_default());
        let advanced_open =
            target.thinking_level.is_some() || target.thinking_format.is_some();
        Self {
            name: target.name.clone(),
            vision: target.vision,
            reasoning: target.reasoning,
            vision_tri: TriToggle::Keep,
            reasoning_tri: TriToggle::Keep,
            initial_context_text: context_text.clone(),
            initial_max_tokens_text: max_tokens_text.clone(),
            context_text,
            max_tokens_text,
            input_cost_text,
            output_cost_text,
            cache_read_cost_text,
            cache_write_cost_text,
            fallback: target.fallback.clone(),
            multi_fallback_mode: MultiFallbackMode::Keep,
            thinking_level,
            thinking_format,
            advanced_open,
            fallback_open: false,
            fallback_list: crate::app::model_picker::ListState::default(),
            error: None,
            initial: Some(target.clone()),
            targets: vec![target],
        }
    }

    pub fn multi(targets: Vec<ModelEditTarget>) -> Self {
        if targets.len() == 1 {
            return Self::single(targets.into_iter().next().unwrap());
        }
        Self {
            targets,
            initial: None,
            initial_context_text: String::new(),
            initial_max_tokens_text: String::new(),
            name: String::new(),
            vision: false,
            reasoning: false,
            vision_tri: TriToggle::Keep,
            reasoning_tri: TriToggle::Keep,
            context_text: String::new(),
            max_tokens_text: String::new(),
            input_cost_text: String::new(),
            output_cost_text: String::new(),
            cache_read_cost_text: String::new(),
            cache_write_cost_text: String::new(),
            fallback: None,
            multi_fallback_mode: MultiFallbackMode::Keep,
            thinking_level: None,
            thinking_format: None,
            advanced_open: false,
            fallback_open: false,
            fallback_list: crate::app::model_picker::ListState::default(),
            error: None,
        }
    }

    pub fn is_multi(&self) -> bool {
        self.targets.len() > 1
    }

    pub fn edited_keys(&self) -> BTreeSet<String> {
        self.targets.iter().map(ModelEditTarget::key).collect()
    }

    pub fn escape(&mut self) -> bool {
        if self.fallback_open {
            self.fallback_open = false;
            return true;
        }
        false
    }

    pub fn build_patch(
        &self,
        existing_fallbacks: &BTreeMap<(String, String), String>,
    ) -> Result<ModelEditPatch, String> {
        let mut patch = ModelEditPatch {
            targets: self
                .targets
                .iter()
                .map(|t| (t.provider.clone(), t.id.clone()))
                .collect(),
            ..Default::default()
        };
        let edited_keys = self.edited_keys();

        if let Some(init) = &self.initial {
            // Single-edit: only persist fields that changed from the initial values.
            let trimmed_name = self.name.trim();
            if trimmed_name.is_empty() {
                return Err("Display name cannot be empty.".into());
            }
            if trimmed_name != init.name {
                patch.name = FieldEdit::Set(trimmed_name.to_owned());
            }
            if self.vision != init.vision {
                patch.vision = FieldEdit::Set(self.vision);
            }
            if self.reasoning != init.reasoning {
                patch.reasoning = FieldEdit::Set(self.reasoning);
            }
            if self.context_text.trim() != self.initial_context_text {
                let ctx = parse_token_count(&self.context_text, 4_096, 10_000_000)
                    .ok_or("Context window must be between 4k and 10M tokens (for example 200k or 1M).")?;
                if ctx != init.context_window.max(4_096) {
                    patch.context_window = FieldEdit::Set(ctx);
                }
            }
            if self.max_tokens_text.trim() != self.initial_max_tokens_text {
                let max_out = parse_token_count(&self.max_tokens_text, 256, 2_000_000)
                    .ok_or("Max output tokens must be between 256 and 2M (for example 16k or 64k).")?;
                if max_out != init.max_tokens.max(1_024) {
                    patch.max_tokens = FieldEdit::Set(max_out);
                }
            }
            let in_cost = parse_price(&self.input_cost_text)
                .ok_or("Input price must be a non-negative number.")?;
            if (in_cost - init.input_cost).abs() > 1e-9 {
                patch.input_cost = FieldEdit::Set(in_cost);
            }
            let out_cost = parse_price(&self.output_cost_text)
                .ok_or("Output price must be a non-negative number.")?;
            if (out_cost - init.output_cost).abs() > 1e-9 {
                patch.output_cost = FieldEdit::Set(out_cost);
            }
            let cr_cost = parse_price(&self.cache_read_cost_text)
                .ok_or("Cache read price must be a non-negative number.")?;
            if (cr_cost - init.cache_read_cost).abs() > 1e-9 {
                patch.cache_read_cost = FieldEdit::Set(cr_cost);
            }
            let cw_cost = parse_price(&self.cache_write_cost_text)
                .ok_or("Cache write price must be a non-negative number.")?;
            if (cw_cost - init.cache_write_cost).abs() > 1e-9 {
                patch.cache_write_cost = FieldEdit::Set(cw_cost);
            }
            if self.fallback != init.fallback {
                match &self.fallback {
                    Some(target) => {
                        validate_fallback_target(&edited_keys, target, existing_fallbacks)?;
                        patch.fallback = FieldEdit::Set(target.clone());
                    }
                    None => patch.fallback = FieldEdit::Clear,
                }
            }
            let next_level = self
                .thinking_level
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if next_level != init.thinking_level {
                patch.thinking_level = match next_level {
                    Some(v) => FieldEdit::Set(v),
                    None => FieldEdit::Clear,
                };
            }
            let next_fmt = self
                .thinking_format
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            if next_fmt != init.thinking_format {
                patch.thinking_format = match next_fmt {
                    Some(v) => FieldEdit::Set(v),
                    None => FieldEdit::Clear,
                };
            }
        } else {
            // Multi-edit: only apply explicitly modified fields.
            patch.vision = match self.vision_tri {
                TriToggle::Keep => FieldEdit::Keep,
                TriToggle::On => FieldEdit::Set(true),
                TriToggle::Off => FieldEdit::Set(false),
            };
            patch.reasoning = match self.reasoning_tri {
                TriToggle::Keep => FieldEdit::Keep,
                TriToggle::On => FieldEdit::Set(true),
                TriToggle::Off => FieldEdit::Set(false),
            };
            if !self.context_text.trim().is_empty() {
                let ctx = parse_token_count(&self.context_text, 4_096, 10_000_000)
                    .ok_or("Context window must be between 4k and 10M tokens (for example 200k or 1M).")?;
                patch.context_window = FieldEdit::Set(ctx);
            }
            if !self.max_tokens_text.trim().is_empty() {
                let max_out = parse_token_count(&self.max_tokens_text, 256, 2_000_000)
                    .ok_or("Max output tokens must be between 256 and 2M (for example 16k or 64k).")?;
                patch.max_tokens = FieldEdit::Set(max_out);
            }
            if !self.input_cost_text.trim().is_empty() {
                let v = parse_price(&self.input_cost_text)
                    .ok_or("Input price must be a non-negative number.")?;
                patch.input_cost = FieldEdit::Set(v);
            }
            if !self.output_cost_text.trim().is_empty() {
                let v = parse_price(&self.output_cost_text)
                    .ok_or("Output price must be a non-negative number.")?;
                patch.output_cost = FieldEdit::Set(v);
            }
            if !self.cache_read_cost_text.trim().is_empty() {
                let v = parse_price(&self.cache_read_cost_text)
                    .ok_or("Cache read price must be a non-negative number.")?;
                patch.cache_read_cost = FieldEdit::Set(v);
            }
            if !self.cache_write_cost_text.trim().is_empty() {
                let v = parse_price(&self.cache_write_cost_text)
                    .ok_or("Cache write price must be a non-negative number.")?;
                patch.cache_write_cost = FieldEdit::Set(v);
            }
            match self.multi_fallback_mode {
                MultiFallbackMode::Keep => {}
                MultiFallbackMode::Clear => patch.fallback = FieldEdit::Clear,
                MultiFallbackMode::Set => {
                    let target = self
                        .fallback
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .ok_or("Choose a fallback model, or switch fallback to Keep or Clear.")?;
                    validate_fallback_target(&edited_keys, target, existing_fallbacks)?;
                    patch.fallback = FieldEdit::Set(target.to_owned());
                }
            }
            if let Some(level) = &self.thinking_level {
                patch.thinking_level = if level.is_empty() {
                    FieldEdit::Clear
                } else {
                    FieldEdit::Set(level.clone())
                };
            }
            if let Some(fmt) = &self.thinking_format {
                patch.thinking_format = if fmt.is_empty() {
                    FieldEdit::Clear
                } else {
                    FieldEdit::Set(fmt.clone())
                };
            }
        }
        Ok(patch)
    }
}

/// Lossless token formatter for editable text fields: abbreviates exact `M` / `k`
/// multiples (`1000000` -> `1M`, `128000` -> `128k`) and keeps exact digits otherwise
/// (`16384` -> `16384`, `1048576` -> `1048576`).
pub(in crate::app) fn format_tokens_exact(tokens: u64) -> String {
    if tokens >= 1_000_000 && tokens.is_multiple_of(1_000_000) {
        format!("{}M", tokens / 1_000_000)
    } else if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}k", tokens / 1_000)
    } else {
        tokens.to_string()
    }
}

pub(in crate::app) fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let m = tokens as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.01 {
            format!("{}M", m.round() as u64)
        } else {
            format!("{m:.1}M")
        }
    } else if tokens >= 1_000 {
        let k = tokens as f64 / 1_000.0;
        if (k - k.round()).abs() < 0.05 {
            format!("{}k", k.round() as u64)
        } else {
            format!("{k:.1}k")
        }
    } else {
        tokens.to_string()
    }
}

pub(in crate::app) fn parse_token_count(input: &str, min: u64, max: u64) -> Option<u64> {
    let s = input.trim().replace([',', '_'], "").to_ascii_lowercase();
    let s = s.strip_suffix("tokens").unwrap_or(&s).trim();
    if s.is_empty() {
        return None;
    }
    let raw = if let Some(num) = s.strip_suffix('m') {
        num.trim().parse::<f64>().ok().map(|v| (v * 1_000_000.0).round())
    } else if let Some(num) = s.strip_suffix('k') {
        num.trim().parse::<f64>().ok().map(|v| (v * 1_000.0).round())
    } else {
        s.parse::<f64>().ok().map(f64::round)
    }?;
    if !raw.is_finite() || raw < min as f64 || raw > max as f64 {
        return None;
    }
    Some(raw as u64)
}

pub(in crate::app) fn format_price(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return "0".into();
    }
    let formatted = format!("{value:.4}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

pub(in crate::app) fn parse_price(input: &str) -> Option<f64> {
    let s = input.trim().trim_start_matches('$').replace(',', "");
    if s.is_empty() {
        return None;
    }
    let v = s.parse::<f64>().ok()?;
    (v.is_finite() && (0.0..=100_000.0).contains(&v)).then_some(v)
}

/// Filter fallback candidates: excludes all currently edited models and any
/// candidate whose configured fallback chain would cycle back to an edited model.
pub(in crate::app) fn fallback_candidates(
    all_models: &[protocol::Model],
    edited_keys: &BTreeSet<String>,
    existing_fallbacks: &BTreeMap<(String, String), String>,
) -> Vec<protocol::Model> {
    all_models
        .iter()
        .filter(|m| {
            let key = format!("{}/{}", m.provider, m.id);
            validate_fallback_target(edited_keys, &key, existing_fallbacks).is_ok()
        })
        .cloned()
        .collect()
}

pub(in crate::app) fn validate_fallback_target(
    edited_keys: &BTreeSet<String>,
    target: &str,
    existing_fallbacks: &BTreeMap<(String, String), String>,
) -> Result<(), String> {
    if edited_keys.contains(target) {
        return Err("A model cannot fall back to itself.".into());
    }
    let mut cursor = target.to_owned();
    let mut seen = BTreeSet::new();
    while seen.insert(cursor.clone()) {
        let Some((prov, id)) = cursor.split_once('/') else {
            break;
        };
        let Some(next) = existing_fallbacks.get(&(prov.to_owned(), id.to_owned())) else {
            break;
        };
        if edited_keys.contains(next) {
            return Err(format!(
                "Cannot set {target} as fallback because its fallback chain loops back."
            ));
        }
        cursor = next.clone();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// JSON persistence helpers (models.json, settings.json, pui-model-fallbacks.json)
// ---------------------------------------------------------------------------

pub(in crate::app) fn read_model_overrides(
    models_root: &Value,
) -> BTreeMap<(String, String), ModelOverride> {
    let mut out = BTreeMap::new();
    let Some(providers) = models_root["providers"].as_object() else {
        return out;
    };
    for (provider, config) in providers {
        let Some(overrides) = config["modelOverrides"].as_object() else {
            continue;
        };
        for (model_id, ov) in overrides {
            let entry = ModelOverride {
                name: ov["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                vision: ov["input"]
                    .as_array()
                    .map(|arr| arr.iter().any(|v| v.as_str() == Some("image"))),
                reasoning: ov["reasoning"].as_bool(),
                context_window: ov["contextWindow"].as_u64().filter(|&v| v > 0),
                max_tokens: ov["maxTokens"].as_u64().filter(|&v| v > 0),
                input_cost: ov["cost"]["input"].as_f64().filter(|v| v.is_finite() && *v >= 0.0),
                output_cost: ov["cost"]["output"].as_f64().filter(|v| v.is_finite() && *v >= 0.0),
                cache_read_cost: ov["cost"]["cacheRead"]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0),
                cache_write_cost: ov["cost"]["cacheWrite"]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.0),
                thinking_format: ov["compat"]["thinkingFormat"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            };
            if !entry.is_empty() {
                out.insert((provider.clone(), model_id.clone()), entry);
            }
        }
    }
    out
}

pub(in crate::app) fn apply_override_patch_to_json(
    root: &mut Value,
    provider: &str,
    model_id: &str,
    patch: &ModelEditPatch,
) -> Result<bool, String> {
    let mut touched = false;
    let has_any_model_json_edit = !matches!(patch.name, FieldEdit::Keep)
        || !matches!(patch.vision, FieldEdit::Keep)
        || !matches!(patch.reasoning, FieldEdit::Keep)
        || !matches!(patch.context_window, FieldEdit::Keep)
        || !matches!(patch.max_tokens, FieldEdit::Keep)
        || !matches!(patch.input_cost, FieldEdit::Keep)
        || !matches!(patch.output_cost, FieldEdit::Keep)
        || !matches!(patch.cache_read_cost, FieldEdit::Keep)
        || !matches!(patch.cache_write_cost, FieldEdit::Keep)
        || !matches!(patch.thinking_format, FieldEdit::Keep);
    if !has_any_model_json_edit {
        return Ok(false);
    }
    if !root["providers"].is_object() {
        root["$schema"] = json!("https://pi.dev/schemas/models.schema.json");
        root["providers"] = json!({});
    }
    let providers = root["providers"]
        .as_object_mut()
        .ok_or("Invalid providers object in models.json.")?;
    let entry = providers
        .entry(provider.to_owned())
        .or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    if !entry["modelOverrides"].is_object() {
        entry["modelOverrides"] = json!({});
    }
    let overrides = entry["modelOverrides"]
        .as_object_mut()
        .ok_or("Invalid modelOverrides object in models.json.")?;
    let model_ov = overrides
        .entry(model_id.to_owned())
        .or_insert_with(|| json!({}));
    if !model_ov.is_object() {
        *model_ov = json!({});
    }
    let obj = model_ov.as_object_mut().unwrap();

    match &patch.name {
        FieldEdit::Set(v) => {
            obj.insert("name".into(), json!(v));
            touched = true;
        }
        FieldEdit::Clear => {
            touched |= obj.remove("name").is_some();
        }
        FieldEdit::Keep => {}
    }
    match patch.vision {
        FieldEdit::Set(true) => {
            obj.insert("input".into(), json!(["text", "image"]));
            touched = true;
        }
        FieldEdit::Set(false) => {
            obj.insert("input".into(), json!(["text"]));
            touched = true;
        }
        FieldEdit::Clear => {
            touched |= obj.remove("input").is_some();
        }
        FieldEdit::Keep => {}
    }
    match patch.reasoning {
        FieldEdit::Set(v) => {
            obj.insert("reasoning".into(), json!(v));
            touched = true;
        }
        FieldEdit::Clear => {
            touched |= obj.remove("reasoning").is_some();
        }
        FieldEdit::Keep => {}
    }
    match patch.context_window {
        FieldEdit::Set(v) => {
            obj.insert("contextWindow".into(), json!(v.clamp(4_096, 10_000_000)));
            touched = true;
        }
        FieldEdit::Clear => {
            touched |= obj.remove("contextWindow").is_some();
        }
        FieldEdit::Keep => {}
    }
    match patch.max_tokens {
        FieldEdit::Set(v) => {
            obj.insert("maxTokens".into(), json!(v.clamp(256, 2_000_000)));
            touched = true;
        }
        FieldEdit::Clear => {
            touched |= obj.remove("maxTokens").is_some();
        }
        FieldEdit::Keep => {}
    }
    for (key, edit) in [
        ("input", &patch.input_cost),
        ("output", &patch.output_cost),
        ("cacheRead", &patch.cache_read_cost),
        ("cacheWrite", &patch.cache_write_cost),
    ] {
        match edit {
            FieldEdit::Set(v) => {
                if !obj.get("cost").is_some_and(Value::is_object) {
                    obj.insert("cost".into(), json!({}));
                }
                obj.get_mut("cost")
                    .and_then(Value::as_object_mut)
                    .unwrap()
                    .insert(key.into(), json!(v));
                touched = true;
            }
            FieldEdit::Clear => {
                if let Some(cost) = obj.get_mut("cost").and_then(Value::as_object_mut) {
                    touched |= cost.remove(key).is_some();
                    if cost.is_empty() {
                        obj.remove("cost");
                    }
                }
            }
            FieldEdit::Keep => {}
        }
    }
    match &patch.thinking_format {
        FieldEdit::Set(v) => {
            if !obj.get("compat").is_some_and(Value::is_object) {
                obj.insert("compat".into(), json!({}));
            }
            obj.get_mut("compat")
                .and_then(Value::as_object_mut)
                .unwrap()
                .insert("thinkingFormat".into(), json!(v));
            touched = true;
        }
        FieldEdit::Clear => {
            if let Some(compat) = obj.get_mut("compat").and_then(Value::as_object_mut) {
                touched |= compat.remove("thinkingFormat").is_some();
                if compat.is_empty() {
                    obj.remove("compat");
                }
            }
        }
        FieldEdit::Keep => {}
    }

    prune_empty_model_override(root, provider, model_id);
    Ok(touched)
}

pub(in crate::app) fn clear_override_in_json(
    root: &mut Value,
    provider: &str,
    model_id: &str,
) -> bool {
    let Some(providers) = root["providers"].as_object_mut() else {
        return false;
    };
    let mut removed = false;
    if let Some(entry) = providers.get_mut(provider).and_then(Value::as_object_mut)
        && let Some(overrides) = entry
            .get_mut("modelOverrides")
            .and_then(Value::as_object_mut)
    {
        removed = overrides.remove(model_id).is_some();
    }
    prune_empty_model_override(root, provider, model_id);
    removed
}

fn prune_empty_model_override(root: &mut Value, provider: &str, model_id: &str) {
    let Some(providers) = root["providers"].as_object_mut() else {
        return;
    };
    let mut remove_provider = false;
    if let Some(entry) = providers.get_mut(provider).and_then(Value::as_object_mut) {
        let mut remove_overrides = false;
        if let Some(overrides) = entry
            .get_mut("modelOverrides")
            .and_then(Value::as_object_mut)
        {
            if overrides
                .get(model_id)
                .and_then(Value::as_object)
                .is_some_and(|m| m.is_empty())
            {
                overrides.remove(model_id);
            }
            remove_overrides = overrides.is_empty();
        }
        if remove_overrides {
            entry.remove("modelOverrides");
        }
        remove_provider = entry.is_empty();
    }
    if remove_provider {
        providers.remove(provider);
    }
}

pub(in crate::app) fn read_model_thinking_levels(
    settings_root: &Value,
) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let Some(map) = settings_root["modelThinkingLevels"].as_object() else {
        return out;
    };
    for (key, val) in map {
        let Some((provider, id)) = key.split_once('/') else {
            continue;
        };
        if let Some(level) = val.as_str().filter(|s| !s.is_empty()) {
            out.insert((provider.to_owned(), id.to_owned()), level.to_owned());
        }
    }
    out
}

pub(in crate::app) fn apply_thinking_level_to_settings(
    settings_root: &mut Value,
    provider: &str,
    model_id: &str,
    edit: &FieldEdit<String>,
) -> bool {
    let key = format!("{provider}/{model_id}");
    match edit {
        FieldEdit::Keep => false,
        FieldEdit::Set(level) => {
            if !settings_root["modelThinkingLevels"].is_object() {
                settings_root["modelThinkingLevels"] = json!({});
            }
            settings_root["modelThinkingLevels"]
                .as_object_mut()
                .unwrap()
                .insert(key, json!(level));
            true
        }
        FieldEdit::Clear => {
            let mut removed = false;
            if let Some(map) = settings_root["modelThinkingLevels"].as_object_mut() {
                removed = map.remove(&key).is_some();
                if map.is_empty()
                    && let Some(root_obj) = settings_root.as_object_mut()
                {
                    root_obj.remove("modelThinkingLevels");
                }
            }
            removed
        }
    }
}

pub(in crate::app) fn read_model_fallbacks(path: &Path) -> BTreeMap<(String, String), String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(root) = serde_json::from_str::<Value>(&text) else {
        return BTreeMap::new();
    };
    parse_model_fallbacks(&root)
}

pub(in crate::app) fn parse_model_fallbacks(root: &Value) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let Some(map) = root["fallbacks"].as_object() else {
        return out;
    };
    for (key, val) in map {
        let Some((provider, id)) = key.split_once('/') else {
            continue;
        };
        let Some(target) = val.as_str().filter(|s| s.contains('/') && *s != key) else {
            continue;
        };
        out.insert((provider.to_owned(), id.to_owned()), target.to_owned());
    }
    out
}

pub(in crate::app) fn serialize_model_fallbacks(
    fallbacks: &BTreeMap<(String, String), String>,
) -> Value {
    let mut map = serde_json::Map::new();
    for ((provider, id), target) in fallbacks {
        map.insert(format!("{provider}/{id}"), json!(target));
    }
    json!({ "fallbacks": map })
}

// ---------------------------------------------------------------------------
// Quiet Mac Edit Model dialog UI
// ---------------------------------------------------------------------------

pub(in crate::app) fn show_model_editor(
    ctx: &egui::Context,
    draft: &mut ModelEditDraft,
    all_models: &[protocol::Model],
    existing_fallbacks: &BTreeMap<(String, String), String>,
) -> EditorOutcome {
    let mut outcome = EditorOutcome::Open;
    let is_multi = draft.is_multi();
    let edited_keys = draft.edited_keys();
    let fallback_choices = fallback_candidates(all_models, &edited_keys, existing_fallbacks);
    let colors = theme::palette(ctx);
    let max_body_height = (ctx.screen_rect().height() - 220.0).clamp(180.0, 520.0);

    let response = prompt_ui::modal(
        ctx,
        egui::Id::new("pui-edit-model-dialog"),
        510.0,
        |ui| {
            let mut close = prompt_ui::close(ui);
            prompt_ui::badge(ui, Icon::Sliders, false);
            if is_multi {
                prompt_ui::heading(
                    ui,
                    &format!("Edit {} Models", draft.targets.len()),
                    "Only fields you change will be updated; untouched fields keep each model's current setting.",
                );
            } else {
                let target = &draft.targets[0];
                let provider_label = crate::accounts::display_provider(&target.provider);
                prompt_ui::heading(
                    ui,
                    "Edit Model",
                    &format!("{provider_label} · {}", target.id),
                );
            }

            if is_multi {
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                    let show_count = draft.targets.len().min(4);
                    for target in &draft.targets[..show_count] {
                        chip(ui, &target.id);
                    }
                    if draft.targets.len() > show_count {
                        chip(ui, &format!("+{} more", draft.targets.len() - show_count));
                    }
                });
            }

            ui.add_space(12.0);
            egui::ScrollArea::vertical()
                .id_salt("pui-edit-model-scroll")
                .max_height(max_body_height)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    // 1. Display name (single-edit only)
                    if !is_multi {
                        field_label(ui, "Display name");
                        ui.add(
                            theme::text_field(&mut draft.name, "Model display name")
                                .id_salt("edit-model-name")
                                .desired_width(ui.available_width()),
                        );
                        ui.add_space(12.0);
                    }

                    // 2. Capabilities
                    group_heading(ui, "CAPABILITIES");
                    sub_panel(ui, |ui| {
                        if is_multi {
                            let vision_summary = summarize_bools(
                                draft.targets.iter().map(|t| t.vision),
                            );
                            tri_toggle_row(
                                ui,
                                "Vision",
                                &format!("Allow image attachments ({vision_summary})"),
                                &mut draft.vision_tri,
                            );
                            panel_divider(ui);
                            let reasoning_summary = summarize_bools(
                                draft.targets.iter().map(|t| t.reasoning),
                            );
                            tri_toggle_row(
                                ui,
                                "Reasoning",
                                &format!("Enable thinking levels ({reasoning_summary})"),
                                &mut draft.reasoning_tri,
                            );
                        } else {
                            capability_row(
                                ui,
                                "Vision",
                                "Allow image attachments and screenshots in chat",
                                &mut draft.vision,
                            );
                            panel_divider(ui);
                            capability_row(
                                ui,
                                "Reasoning",
                                "Enable thinking levels and reasoning traces",
                                &mut draft.reasoning,
                            );
                        }
                    });

                    ui.add_space(12.0);

                    // 3. Limits
                    group_heading(ui, "LIMITS");
                    sub_panel(ui, |ui| {
                        let col_w = ((ui.available_width() - 12.0) / 2.0).max(110.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                field_label(ui, "Context window");
                                let hint = if is_multi {
                                    "Keep existing (e.g. 200k, 1M)"
                                } else {
                                    "e.g. 200k, 1M"
                                };
                                ui.add(
                                    theme::text_field(&mut draft.context_text, hint)
                                        .id_salt("edit-model-context")
                                        .desired_width(col_w),
                                );
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                    for preset in ["128k", "200k", "1M"] {
                                        let active = draft
                                            .context_text
                                            .trim()
                                            .eq_ignore_ascii_case(preset);
                                        if theme::choice(ui, active, preset).clicked() {
                                            draft.context_text = preset.into();
                                        }
                                    }
                                });
                            });
                            ui.vertical(|ui| {
                                ui.set_width(col_w);
                                field_label(ui, "Max output tokens");
                                let hint = if is_multi {
                                    "Keep existing (e.g. 16k, 64k)"
                                } else {
                                    "e.g. 16k, 64k"
                                };
                                ui.add(
                                    theme::text_field(&mut draft.max_tokens_text, hint)
                                        .id_salt("edit-model-max-tokens")
                                        .desired_width(col_w),
                                );
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                    for preset in ["16k", "64k", "128k"] {
                                        let active = draft
                                            .max_tokens_text
                                            .trim()
                                            .eq_ignore_ascii_case(preset);
                                        if theme::choice(ui, active, preset).clicked() {
                                            draft.max_tokens_text = preset.into();
                                        }
                                    }
                                });
                            });
                        });
                    });

                    ui.add_space(12.0);

                    // 4. Fallback model (uses Pui's shared searchable model dropdown)
                    group_heading(ui, "FALLBACK ON RATE LIMIT / UNAVAILABLE");
                    sub_panel(ui, |ui| {
                        field_label(
                            ui,
                            "Switch chat to this model on 429 / quota / 503 before output",
                        );
                        if is_multi {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                for (mode, label) in [
                                    (MultiFallbackMode::Keep, "Keep existing"),
                                    (MultiFallbackMode::Set, "Set fallback"),
                                    (MultiFallbackMode::Clear, "Clear fallback"),
                                ] {
                                    if theme::choice(ui, draft.multi_fallback_mode == mode, label)
                                        .clicked()
                                    {
                                        draft.multi_fallback_mode = mode;
                                        if mode != MultiFallbackMode::Set {
                                            draft.fallback_open = false;
                                        }
                                    }
                                }
                            });
                            if draft.multi_fallback_mode == MultiFallbackMode::Set {
                                ui.add_space(6.0);
                                let pick = page::model_dropdown(
                                    ui,
                                    &mut draft.fallback_open,
                                    &mut draft.fallback_list,
                                    page::Dropdown {
                                        salt: "edit-model-fallback-multi",
                                        title: "Fallback model",
                                        models: &fallback_choices,
                                        selected: draft.fallback.as_deref(),
                                        automatic: None,
                                        empty: "No other models available as fallback.",
                                    },
                                );
                                if let Some(crate::app::model_picker::Pick::Model {
                                    provider: prov,
                                    id,
                                }) = pick
                                {
                                    draft.fallback = Some(format!("{prov}/{id}"));
                                }
                            }
                        } else {
                            let pick = page::model_dropdown(
                                ui,
                                &mut draft.fallback_open,
                                &mut draft.fallback_list,
                                page::Dropdown {
                                    salt: "edit-model-fallback",
                                    title: "Fallback model",
                                    models: &fallback_choices,
                                    selected: draft.fallback.as_deref(),
                                    automatic: Some((
                                        "None",
                                        "No automatic fallback; fail after normal retries.",
                                    )),
                                    empty: "No other models available as fallback.",
                                },
                            );
                            match pick {
                                Some(crate::app::model_picker::Pick::Automatic) => {
                                    draft.fallback = None;
                                }
                                Some(crate::app::model_picker::Pick::Model {
                                    provider: prov,
                                    id,
                                }) => {
                                    draft.fallback = Some(format!("{prov}/{id}"));
                                }
                                None => {}
                            }
                        }
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    "Tries extra accounts of the same provider first, then switches the chat to this model with a notice. Currently edited model(s) are excluded.",
                                )
                                .size(11.0)
                                .color(colors.dim),
                            )
                            .wrap(),
                        );
                    });

                    ui.add_space(12.0);

                    // 5. Pricing ($ per 1M tokens)
                    group_heading(ui, "PRICING ($ PER 1M TOKENS)");
                    sub_panel(ui, |ui| {
                        let col_w = ((ui.available_width() - 24.0) / 4.0).max(58.0);
                        let hint = if is_multi { "Keep" } else { "0" };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            for (label, salt, value) in [
                                ("Input", "edit-model-price-in", &mut draft.input_cost_text),
                                ("Output", "edit-model-price-out", &mut draft.output_cost_text),
                                (
                                    "Cache read",
                                    "edit-model-price-cr",
                                    &mut draft.cache_read_cost_text,
                                ),
                                (
                                    "Cache write",
                                    "edit-model-price-cw",
                                    &mut draft.cache_write_cost_text,
                                ),
                            ] {
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    field_label(ui, label);
                                    ui.add(
                                        theme::text_field(value, hint)
                                            .id_salt(salt)
                                            .desired_width(col_w),
                                    );
                                });
                            }
                        });
                    });

                    ui.add_space(12.0);

                    // 6. Collapsible Advanced
                    let disclosure_label = if draft.advanced_open {
                        "▾ Advanced (Default thinking level, Reasoning format)"
                    } else {
                        "▸ Advanced (Default thinking level, Reasoning format)"
                    };
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new(disclosure_label)
                                    .size(12.0)
                                    .color(colors.muted),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        draft.advanced_open = !draft.advanced_open;
                    }
                    if draft.advanced_open {
                        ui.add_space(6.0);
                        sub_panel(ui, |ui| {
                            let col_w = ((ui.available_width() - 12.0) / 2.0).max(110.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    field_label(ui, "Default thinking level");
                                    let selected_text = match &draft.thinking_level {
                                        None => "Keep existing",
                                        Some(val) => THINKING_LEVEL_OPTIONS
                                            .iter()
                                            .find(|(k, _)| *k == val.as_str())
                                            .map(|(_, l)| *l)
                                            .unwrap_or(val.as_str()),
                                    };
                                    egui::ComboBox::from_id_salt("edit-model-thinking-level")
                                        .selected_text(selected_text)
                                        .width(col_w)
                                        .show_ui(ui, |ui| {
                                            if is_multi
                                                && ui
                                                    .selectable_label(
                                                        draft.thinking_level.is_none(),
                                                        "Keep existing",
                                                    )
                                                    .clicked()
                                            {
                                                draft.thinking_level = None;
                                            }
                                            for (key, label) in THINKING_LEVEL_OPTIONS {
                                                let active = draft.thinking_level.as_deref()
                                                    == Some(key);
                                                if ui.selectable_label(active, label).clicked() {
                                                    draft.thinking_level = Some(key.to_owned());
                                                }
                                            }
                                        });
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(col_w);
                                    field_label(ui, "Reasoning format (compat)");
                                    let selected_text = match &draft.thinking_format {
                                        None => "Keep existing",
                                        Some(val) => THINKING_FORMAT_OPTIONS
                                            .iter()
                                            .find(|(k, _)| *k == val.as_str())
                                            .map(|(_, l)| *l)
                                            .unwrap_or(val.as_str()),
                                    };
                                    egui::ComboBox::from_id_salt("edit-model-thinking-format")
                                        .selected_text(selected_text)
                                        .width(col_w)
                                        .show_ui(ui, |ui| {
                                            if is_multi
                                                && ui
                                                    .selectable_label(
                                                        draft.thinking_format.is_none(),
                                                        "Keep existing",
                                                    )
                                                    .clicked()
                                            {
                                                draft.thinking_format = None;
                                            }
                                            for (key, label) in THINKING_FORMAT_OPTIONS {
                                                let active = draft.thinking_format.as_deref()
                                                    == Some(key);
                                                if ui.selectable_label(active, label).clicked() {
                                                    draft.thinking_format = Some(key.to_owned());
                                                }
                                            }
                                        });
                                });
                            });
                        });
                    }
                });

            if let Some(err) = &draft.error {
                ui.add_space(8.0);
                ui.add(
                    egui::Label::new(RichText::new(err).size(11.5).color(colors.red)).wrap(),
                );
            }

            ui.add_space(14.0);
            let save_label = if is_multi {
                format!("Apply to {} models", draft.targets.len())
            } else {
                "Save".to_owned()
            };
            let reset_label = if is_multi {
                "Reset all to defaults"
            } else {
                "Reset to defaults"
            };
            let any_customized = draft.targets.iter().any(|t| t.has_customizations);
            let mut trigger_save = false;
            let mut trigger_reset = false;
            ui.horizontal(|ui| {
                if prompt_ui::button(
                    ui,
                    reset_label,
                    prompt_ui::Button::Secondary,
                    any_customized,
                )
                .clicked()
                {
                    trigger_reset = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if prompt_ui::button(ui, &save_label, prompt_ui::Button::Primary, true)
                        .clicked()
                    {
                        trigger_save = true;
                    }
                    if prompt_ui::button(ui, "Cancel", prompt_ui::Button::Secondary, true)
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            (close, trigger_save, trigger_reset)
        },
    );

    let (close, trigger_save, trigger_reset) = response.inner;
    if trigger_reset {
        let targets = draft
            .targets
            .iter()
            .map(|t| (t.provider.clone(), t.id.clone()))
            .collect();
        outcome = EditorOutcome::Reset(targets);
    } else if trigger_save {
        match draft.build_patch(existing_fallbacks) {
            Ok(patch) => {
                draft.error = None;
                outcome = EditorOutcome::Save(patch);
            }
            Err(err) => {
                draft.error = Some(err);
            }
        }
    } else if close || (response.should_close() && !draft.fallback_open) {
        outcome = EditorOutcome::Cancel;
    }
    outcome
}

fn group_heading(ui: &mut egui::Ui, text: &str) {
    let colors = theme::palette(ui.ctx());
    ui.label(
        RichText::new(text)
            .size(10.5)
            .strong()
            .color(colors.dim),
    );
    ui.add_space(4.0);
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    let colors = theme::palette(ui.ctx());
    ui.label(RichText::new(text).size(11.5).color(colors.muted));
    ui.add_space(3.0);
}

fn sub_panel(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(prompt_ui::subtle(ui.ctx()))
        .stroke(Stroke::new(1.0_f32, prompt_ui::border(ui.ctx())))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

fn panel_divider(ui: &mut egui::Ui) {
    let colors = theme::palette(ui.ctx());
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0_f32, colors.border.gamma_multiply(0.7)),
    );
    ui.add_space(6.0);
}

fn chip(ui: &mut egui::Ui, text: &str) {
    let colors = theme::palette(ui.ctx());
    egui::Frame::new()
        .fill(prompt_ui::subtle(ui.ctx()))
        .stroke(Stroke::new(1.0_f32, prompt_ui::border(ui.ctx())))
        .corner_radius(5)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(11.0)
                    .monospace()
                    .color(colors.muted),
            );
        });
}

fn capability_row(ui: &mut egui::Ui, title: &str, description: &str, value: &mut bool) {
    let colors = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        let label_w = (ui.available_width() - 48.0).max(100.0);
        ui.vertical(|ui| {
            ui.set_width(label_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(12.5).strong().color(colors.text));
            ui.add(
                egui::Label::new(RichText::new(description).size(11.0).color(colors.muted)).wrap(),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            page::toggle(ui, value, title);
        });
    });
}

fn tri_toggle_row(ui: &mut egui::Ui, title: &str, description: &str, value: &mut TriToggle) {
    let colors = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        let label_w = (ui.available_width() - 160.0).max(90.0);
        ui.vertical(|ui| {
            ui.set_width(label_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(12.5).strong().color(colors.text));
            ui.add(
                egui::Label::new(RichText::new(description).size(11.0).color(colors.muted)).wrap(),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (state, label) in [
                (TriToggle::Off, "Off"),
                (TriToggle::On, "On"),
                (TriToggle::Keep, "Keep"),
            ] {
                if theme::choice(ui, *value == state, label).clicked() {
                    *value = state;
                }
            }
        });
    });
}

fn summarize_bools(mut values: impl Iterator<Item = bool>) -> &'static str {
    let Some(first) = values.next() else {
        return "none";
    };
    if values.all(|v| v == first) {
        if first { "currently On" } else { "currently Off" }
    } else {
        "currently mixed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(provider: &str, id: &str, vision: bool, reasoning: bool) -> ModelEditTarget {
        ModelEditTarget {
            provider: provider.into(),
            id: id.into(),
            name: id.into(),
            vision,
            reasoning,
            context_window: 128_000,
            max_tokens: 16_384,
            input_cost: 0.0,
            output_cost: 0.0,
            cache_read_cost: 0.0,
            cache_write_cost: 0.0,
            thinking_format: None,
            thinking_level: None,
            fallback: None,
            has_customizations: false,
        }
    }

    #[test]
    fn single_edit_only_writes_changed_fields_and_roundtrips_json() {
        let mut draft = ModelEditDraft::single(target("Nvidia", "z-ai/glm-5.3-flash", false, false));
        draft.vision = true;
        draft.reasoning = true;
        draft.context_text = "1M".into();
        draft.fallback = Some("anthropic/claude-sonnet-4-6".into());
        draft.thinking_level = Some("high".into());
        draft.thinking_format = Some("zai".into());

        let existing = BTreeMap::new();
        let patch = draft.build_patch(&existing).expect("valid patch");
        assert_eq!(patch.name, FieldEdit::Keep);
        assert_eq!(patch.vision, FieldEdit::Set(true));
        assert_eq!(patch.reasoning, FieldEdit::Set(true));
        assert_eq!(patch.context_window, FieldEdit::Set(1_000_000));
        assert_eq!(patch.max_tokens, FieldEdit::Keep);
        assert_eq!(
            patch.fallback,
            FieldEdit::Set("anthropic/claude-sonnet-4-6".into())
        );
        assert_eq!(patch.thinking_level, FieldEdit::Set("high".into()));
        assert_eq!(patch.thinking_format, FieldEdit::Set("zai".into()));

        let mut models_root = json!({
            "providers": {
                "Nvidia": {
                    "baseUrl": "https://integrate.api.nvidia.com/v1",
                    "modelOverrides": {
                        "z-ai/glm-5.3-flash": {
                            "compat": { "supportsStore": false }
                        }
                    }
                }
            }
        });
        assert!(apply_override_patch_to_json(
            &mut models_root,
            "Nvidia",
            "z-ai/glm-5.3-flash",
            &patch
        )
        .unwrap());
        let ov = &models_root["providers"]["Nvidia"]["modelOverrides"]["z-ai/glm-5.3-flash"];
        assert_eq!(ov["input"], json!(["text", "image"]));
        assert_eq!(ov["reasoning"], json!(true));
        assert_eq!(ov["contextWindow"], json!(1_000_000));
        assert_eq!(ov["compat"]["thinkingFormat"], json!("zai"));
        // Preserves unrelated compat fields!
        assert_eq!(ov["compat"]["supportsStore"], json!(false));

        let parsed = read_model_overrides(&models_root);
        let parsed_ov = &parsed[&("Nvidia".into(), "z-ai/glm-5.3-flash".into())];
        assert_eq!(parsed_ov.vision, Some(true));
        assert_eq!(parsed_ov.reasoning, Some(true));
        assert_eq!(parsed_ov.context_window, Some(1_000_000));
        assert_eq!(parsed_ov.thinking_format.as_deref(), Some("zai"));

        // Clear removes modelOverrides cleanly while keeping provider baseUrl.
        assert!(clear_override_in_json(
            &mut models_root,
            "Nvidia",
            "z-ai/glm-5.3-flash"
        ));
        assert!(models_root["providers"]["Nvidia"].get("modelOverrides").is_none());
        assert_eq!(
            models_root["providers"]["Nvidia"]["baseUrl"],
            json!("https://integrate.api.nvidia.com/v1")
        );
    }

    #[test]
    fn multi_edit_keeps_untouched_fields_and_excludes_self_and_cycles() {
        let t1 = target("Nvidia", "m1", true, false);
        let t2 = target("Nvidia", "m2", false, true);
        let mut draft = ModelEditDraft::multi(vec![t1, t2]);
        draft.vision_tri = TriToggle::On;
        draft.context_text = "256k".into();
        draft.multi_fallback_mode = MultiFallbackMode::Set;
        draft.fallback = Some("anthropic/sonnet".into());

        let mut existing = BTreeMap::new();
        // Existing chain: google/pro -> Nvidia/m1 (would cycle if Nvidia/m1 falls back to google/pro!)
        existing.insert(("google".into(), "pro".into()), "Nvidia/m1".into());

        let all = vec![
            protocol::Model {
                id: "m1".into(),
                name: "M1".into(),
                provider: "Nvidia".into(),
                context_window: 128_000,
                input_cost: 0.0,
                output_cost: 0.0,
            },
            protocol::Model {
                id: "m2".into(),
                name: "M2".into(),
                provider: "Nvidia".into(),
                context_window: 128_000,
                input_cost: 0.0,
                output_cost: 0.0,
            },
            protocol::Model {
                id: "pro".into(),
                name: "Gemini Pro".into(),
                provider: "google".into(),
                context_window: 1_000_000,
                input_cost: 1.25,
                output_cost: 5.0,
            },
            protocol::Model {
                id: "sonnet".into(),
                name: "Claude Sonnet".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
                input_cost: 3.0,
                output_cost: 15.0,
            },
        ];

        let candidates = fallback_candidates(&all, &draft.edited_keys(), &existing);
        let candidate_keys: Vec<_> = candidates
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect();
        // Excludes m1, m2 (self) AND google/pro (cycle), keeps anthropic/sonnet!
        assert_eq!(candidate_keys, vec!["anthropic/sonnet"]);

        let patch = draft.build_patch(&existing).expect("valid multi patch");
        assert_eq!(patch.vision, FieldEdit::Set(true));
        assert_eq!(patch.reasoning, FieldEdit::Keep);
        assert_eq!(patch.context_window, FieldEdit::Set(256_000));
        assert_eq!(patch.max_tokens, FieldEdit::Keep);
        assert_eq!(patch.fallback, FieldEdit::Set("anthropic/sonnet".into()));

        // Rejects self-fallback directly
        draft.fallback = Some("Nvidia/m2".into());
        assert!(draft.build_patch(&existing).is_err());

        // Rejects cycle directly
        draft.fallback = Some("google/pro".into());
        assert!(draft.build_patch(&existing).is_err());
    }

    #[test]
    fn thinking_level_persists_and_cleans_up_in_settings_json() {
        let mut settings = json!({ "defaultThinkingLevel": "medium" });
        assert!(apply_thinking_level_to_settings(
            &mut settings,
            "Nvidia",
            "glm",
            &FieldEdit::Set("high".into())
        ));
        let map = read_model_thinking_levels(&settings);
        assert_eq!(
            map.get(&("Nvidia".into(), "glm".into())).map(String::as_str),
            Some("high")
        );
        assert!(apply_thinking_level_to_settings(
            &mut settings,
            "Nvidia",
            "glm",
            &FieldEdit::Clear
        ));
        assert!(settings.get("modelThinkingLevels").is_none());
        assert_eq!(settings["defaultThinkingLevel"], json!("medium"));
    }

    #[test]
    fn fallbacks_json_ignores_self_entries_and_roundtrips() {
        let mut map = BTreeMap::new();
        map.insert(
            ("antigravity".into(), "fable-5.2".into()),
            "anthropic/claude-sonnet-4-6".into(),
        );
        let serialized = serialize_model_fallbacks(&map);
        let parsed = parse_model_fallbacks(&serialized);
        assert_eq!(parsed, map);

        let with_self = json!({
            "fallbacks": {
                "antigravity/fable-5.2": "antigravity/fable-5.2",
                "Nvidia/glm": "anthropic/claude-sonnet-4-6"
            }
        });
        let parsed = parse_model_fallbacks(&with_self);
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed
                .get(&("Nvidia".into(), "glm".into()))
                .map(String::as_str),
            Some("anthropic/claude-sonnet-4-6")
        );
    }

    #[test]
    fn edit_model_dialog_renders_cleanly_in_light_dark_narrow_zoom_and_cancels_on_escape() {
        let all_models = vec![
            protocol::Model {
                id: "z-ai/glm-5.3-flash".into(),
                name: "z-ai/glm-5.3-flash".into(),
                provider: "Nvidia".into(),
                context_window: 1_000_000,
                input_cost: 0.0,
                output_cost: 0.0,
            },
            protocol::Model {
                id: "claude-sonnet-4-6".into(),
                name: "Claude Sonnet 4.6".into(),
                provider: "anthropic".into(),
                context_window: 200_000,
                input_cost: 3.0,
                output_cost: 15.0,
            },
        ];
        for mode in [theme::Theme::Light, theme::Theme::Dark] {
            for (width, zoom) in [(520.0, 1.25), (900.0, 1.0)] {
                let ctx = egui::Context::default();
                mode.apply(&ctx);
                ctx.set_zoom_factor(zoom);
                let mut draft = ModelEditDraft::single(target(
                    "Nvidia",
                    "z-ai/glm-5.3-flash",
                    true,
                    true,
                ));
                let existing = BTreeMap::new();
                let mut outcome = EditorOutcome::Open;
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 760.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        outcome = show_model_editor(ctx, &mut draft, &all_models, &existing);
                    },
                );
                assert!(matches!(outcome, EditorOutcome::Open));

                // Multi-edit renders cleanly as well and cancels on Escape.
                let mut multi = ModelEditDraft::multi(vec![
                    target("Nvidia", "z-ai/glm-5.3-flash", true, false),
                    target("anthropic", "claude-sonnet-4-6", true, true),
                ]);
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 760.0),
                        )),
                        events: vec![egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }],
                        ..Default::default()
                    },
                    |ctx| {
                        outcome = show_model_editor(ctx, &mut multi, &all_models, &existing);
                    },
                );
                assert!(matches!(outcome, EditorOutcome::Cancel));
            }
        }
    }
}

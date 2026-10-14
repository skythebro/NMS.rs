//! The mods loaded in the game and their settings, as the settings window sees them.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A switch the player can flip live.
    Bool,
    /// Read-only text such as the Status line.
    Status,
    /// A text value the player can edit.
    Text,
    /// A whole number inside the mod's [`Range::Int`].
    Int,
    /// A decimal number inside the mod's [`Range::Float`].
    Float,
    /// One of a fixed list of choices, stored as the choice's key ([`Range::Enum`]).
    Enum,
}

/// The limits of a number or choice setting, from [`GameMod::range`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Range {
    Int { min: i32, max: i32, step: i32, default: i32 },
    Float { min: f32, max: f32, step: f32, default: f32 },
    /// `(key, label)` per choice; `default` is an index into `choices`.
    Enum { choices: &'static [(&'static str, &'static str)], default: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingSpec {
    pub key: &'static str,
    /// Short: the settings window draws two equal columns of unwrapped text.
    pub label: &'static str,
    pub kind: Kind,
}

/// One mod as the core and the settings window see it.
pub trait GameMod: Send + Sync {
    /// Folder name under `GAMEDATA\MODS`, e.g. `CraftFromStorage`.
    fn name(&self) -> &'static str;
    fn specs(&self) -> &'static [SettingSpec];
    /// Limits for an `Int`, `Float` or `Enum` setting. `None` for every other kind.
    fn range(&self, _key: &str) -> Option<Range> {
        None
    }
    fn get(&self, key: &str) -> Option<Value>;
    /// The shipped default of one setting, for Reset. `None` for Status settings.
    fn default_value(&self, _key: &str) -> Option<Value> {
        None
    }
    /// Save the value and apply it live. Returns the line to show and log.
    fn set(&self, key: &str, value: &Value) -> Result<String, String>;
}

#[derive(Default)]
pub struct Registry {
    mods: Vec<Box<dyn GameMod>>,
}

#[derive(Serialize)]
struct SettingView {
    key: &'static str,
    label: &'static str,
    kind: Kind,
    value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    range: Option<Range>,
}

#[derive(Serialize)]
struct ModView {
    name: &'static str,
    settings: Vec<SettingView>,
}

impl Registry {
    pub fn add(&mut self, m: Box<dyn GameMod>) {
        self.mods.push(m);
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.mods.iter().map(|m| m.name()).collect()
    }

    pub fn list(&self) -> Value {
        let views: Vec<ModView> = self
            .mods
            .iter()
            .map(|m| ModView {
                name: m.name(),
                settings: m
                    .specs()
                    .iter()
                    .map(|s| SettingView { key: s.key, label: s.label, kind: s.kind, value: m.get(s.key).unwrap_or(Value::Null), range: m.range(s.key) })
                    .collect(),
            })
            .collect();
        serde_json::to_value(views).unwrap_or(Value::Null)
    }

    /// Every mod with its settings, in registration order.
    pub fn all_specs(&self) -> Vec<(&'static str, &'static [SettingSpec])> {
        self.mods.iter().map(|m| (m.name(), m.specs())).collect()
    }

    /// The current value of one setting, for code that draws a single row (the in-game menu).
    pub fn get(&self, mod_name: &str, key: &str) -> Option<Value> {
        self.mods.iter().find(|m| m.name() == mod_name)?.get(key)
    }

    /// The shipped default of one setting.
    pub fn default_value(&self, mod_name: &str, key: &str) -> Option<Value> {
        self.mods.iter().find(|m| m.name() == mod_name)?.default_value(key)
    }

    /// The limits of one number or choice setting.
    pub fn range(&self, mod_name: &str, key: &str) -> Option<Range> {
        self.mods.iter().find(|m| m.name() == mod_name)?.range(key)
    }

    pub fn set(&self, mod_name: &str, key: &str, value: &Value) -> Result<String, String> {
        let m = self.mods.iter().find(|m| m.name() == mod_name).ok_or_else(|| format!("no mod {mod_name:?}"))?;
        let spec = m.specs().iter().find(|s| s.key == key).ok_or_else(|| format!("{mod_name} has no setting {key:?}"))?;
        match (spec.kind, value) {
            (Kind::Status, _) => Err(format!("{key} is read-only")),
            (Kind::Bool, Value::Bool(_)) | (Kind::Text, Value::String(_)) => m.set(key, value),
            (Kind::Int, Value::Number(n)) => match (n.as_i64(), m.range(key)) {
                (Some(v), Some(Range::Int { min, max, .. })) if v >= i64::from(min) && v <= i64::from(max) => m.set(key, value),
                (Some(_), Some(Range::Int { min, max, .. })) => Err(format!("{key} must be {min} to {max}")),
                _ => Err(format!("{key} needs a whole number")),
            },
            (Kind::Float, Value::Number(n)) => match (n.as_f64(), m.range(key)) {
                (Some(v), Some(Range::Float { min, max, .. })) if v >= f64::from(min) && v <= f64::from(max) => m.set(key, value),
                (Some(_), Some(Range::Float { min, max, .. })) => Err(format!("{key} must be {min} to {max}")),
                _ => Err(format!("{key} needs a number")),
            },
            (Kind::Enum, Value::String(choice)) => match m.range(key) {
                Some(Range::Enum { choices, .. }) if choices.iter().any(|(k, _)| k == choice) => m.set(key, value),
                Some(Range::Enum { choices, .. }) => {
                    Err(format!("{key} must be one of {}", choices.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")))
                }
                _ => Err(format!("{key} has no choices")),
            },
            _ => Err(format!("wrong value type for {key}")),
        }
    }
}

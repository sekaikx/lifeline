//! Providers, model roles and where they are stored.
//!
//! Settings live in `ai.json` in the platform config directory
//! (`~/.config/lifeline` on Linux), readable by the user only. API keys are
//! kept there and never sent back to the interface: the UI sees whether a
//! key is set, not the key.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// `POST {base}/chat/completions`, the API nearly every server speaks
    #[default]
    OpenAi,
    /// Anthropic Messages API
    Anthropic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ProviderKind,
    /// e.g. `http://localhost:11434/v1`
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    /// Runs on this machine or network: data never leaves it
    #[serde(default)]
    pub local: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Analyst conversation and written reports
    Chat,
    /// Reading satellite images
    Vision,
    /// Small structured jobs
    Fast,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub chat: Option<ModelRef>,
    #[serde(default)]
    pub vision: Option<ModelRef>,
    #[serde(default)]
    pub fast: Option<ModelRef>,
    /// Only local providers may be used, whatever else is configured
    #[serde(default)]
    pub local_only: bool,
}

/// A provider as the UI sees it: the key replaced by whether there is one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PublicProvider {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub local: bool,
    pub has_key: bool,
    /// From the UI only: `Some("")` clears the key, `None` keeps it
    #[serde(default, skip_serializing)]
    pub new_key: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PublicSettings {
    pub enabled: bool,
    pub providers: Vec<PublicProvider>,
    pub chat: Option<ModelRef>,
    pub vision: Option<ModelRef>,
    pub fast: Option<ModelRef>,
    pub local_only: bool,
    /// Where the settings are stored
    #[serde(default)]
    pub path: Option<String>,
}

impl AiSettings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("lifeline").join("ai.json"))
    }

    /// The stored settings, or the defaults if there are none.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory on this system")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.part");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
    }

    pub fn public(&self) -> PublicSettings {
        PublicSettings {
            enabled: self.enabled,
            providers: self
                .providers
                .iter()
                .map(|p| PublicProvider {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    kind: p.kind,
                    base_url: p.base_url.clone(),
                    local: p.local,
                    has_key: p.api_key.as_deref().is_some_and(|k| !k.is_empty()),
                    new_key: None,
                })
                .collect(),
            chat: self.chat.clone(),
            vision: self.vision.clone(),
            fast: self.fast.clone(),
            local_only: self.local_only,
            path: Self::path().map(|p| p.display().to_string()),
        }
    }

    /// Apply settings edited in the UI, keeping stored keys it did not change.
    pub fn merge(&self, ui: PublicSettings) -> Result<Self, String> {
        let mut providers = Vec::new();
        for p in ui.providers {
            if p.id.trim().is_empty() || p.name.trim().is_empty() {
                return Err("every provider needs an id and a name".into());
            }
            let url = p.base_url.trim().trim_end_matches('/').to_string();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(format!(
                    "{}: the address must start with http:// or https://",
                    p.name
                ));
            }
            if providers.iter().any(|q: &Provider| q.id == p.id) {
                return Err(format!("two providers share the id {}", p.id));
            }
            let old = self.providers.iter().find(|q| q.id == p.id);
            let api_key = match p.new_key {
                Some(k) if k.trim().is_empty() => None,
                Some(k) => Some(k.trim().to_string()),
                None => old.and_then(|o| o.api_key.clone()),
            };
            providers.push(Provider {
                id: p.id,
                name: p.name,
                kind: p.kind,
                base_url: url,
                api_key,
                local: p.local,
            });
        }
        let known = |m: &Option<ModelRef>| -> Result<(), String> {
            match m {
                Some(m) if !providers.iter().any(|p| p.id == m.provider) => Err(format!(
                    "model {} uses an unknown provider {}",
                    m.model, m.provider
                )),
                _ => Ok(()),
            }
        };
        known(&ui.chat)?;
        known(&ui.vision)?;
        known(&ui.fast)?;
        Ok(AiSettings {
            enabled: ui.enabled,
            providers,
            chat: ui.chat,
            vision: ui.vision,
            fast: ui.fast,
            local_only: ui.local_only,
        })
    }

    /// The provider and model for a role. `fast` falls back to `chat`, and
    /// `chat` to `vision`, so one capable model is enough to start.
    pub fn resolve(&self, role: Role) -> Result<(&Provider, &str), String> {
        if !self.enabled {
            return Err("the assistant is off; turn it on in AI settings".into());
        }
        let pick = match role {
            Role::Fast => self.fast.as_ref().or(self.chat.as_ref()),
            Role::Chat => self.chat.as_ref().or(self.vision.as_ref()),
            Role::Vision => self.vision.as_ref(),
        };
        let m = pick.ok_or(match role {
            Role::Vision => "choose a vision model in AI settings to read images",
            _ => "choose a chat model in AI settings",
        })?;
        let p = self
            .providers
            .iter()
            .find(|p| p.id == m.provider)
            .ok_or_else(|| format!("provider {} is not configured", m.provider))?;
        if self.local_only && !p.local {
            return Err(format!(
                "{} is a cloud provider and the assistant is set to local models only",
                p.name
            ));
        }
        Ok((p, m.model.as_str()))
    }
}

/// A ready-made provider with suggested models for each role.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub local: bool,
    pub needs_key: bool,
    /// Costs nothing to use (a free tier, or local)
    pub free: bool,
    /// Where to get a key, or the software
    pub url: String,
    pub chat: Option<String>,
    pub vision: Option<String>,
    pub fast: Option<String>,
    pub note: String,
}

#[allow(clippy::too_many_arguments)]
fn preset(
    id: &str,
    name: &str,
    kind: ProviderKind,
    base_url: &str,
    (local, needs_key, free): (bool, bool, bool),
    url: &str,
    [chat, vision, fast]: [&str; 3],
    note: &str,
) -> Preset {
    let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
    Preset {
        id: id.into(),
        name: name.into(),
        kind,
        base_url: base_url.into(),
        local,
        needs_key,
        free,
        url: url.into(),
        chat: opt(chat),
        vision: opt(vision),
        fast: opt(fast),
        note: note.into(),
    }
}

/// Providers Lifeline knows how to set up. Suggested model names are a
/// starting point; "Fetch models" lists what a provider serves today.
pub fn presets() -> Vec<Preset> {
    use ProviderKind::{Anthropic, OpenAi};
    vec![
        preset(
            "ollama",
            "Ollama (this computer)",
            OpenAi,
            "http://localhost:11434/v1",
            (true, false, true),
            "https://ollama.com/download",
            ["gemma3:12b", "qwen2.5vl:7b", "qwen3:4b"],
            "Runs offline once models are pulled, e.g. `ollama pull gemma3:12b`. Gemma 3 reads images and text; Qwen2.5-VL is strong on maps and imagery; Qwen3 4B handles small jobs quickly.",
        ),
        preset(
            "lmstudio",
            "LM Studio (this computer)",
            OpenAi,
            "http://localhost:1234/v1",
            (true, false, true),
            "https://lmstudio.ai",
            ["", "", ""],
            "Start the local server in LM Studio, load a model, then fetch the model list here.",
        ),
        preset(
            "llamacpp",
            "llama.cpp / vLLM server",
            OpenAi,
            "http://localhost:8080/v1",
            (true, false, true),
            "https://github.com/ggml-org/llama.cpp",
            ["", "", ""],
            "Any OpenAI-compatible server on your machine or network. Change the address to match.",
        ),
        preset(
            "gemini",
            "Google Gemini (free tier)",
            OpenAi,
            "https://generativelanguage.googleapis.com/v1beta/openai",
            (false, true, true),
            "https://aistudio.google.com/apikey",
            ["gemini-2.5-flash", "gemini-2.5-flash", "gemini-2.5-flash-lite"],
            "Free key from Google AI Studio. Strong at reading images; the free tier has daily limits and Google may use free-tier prompts to improve its products.",
        ),
        preset(
            "openrouter",
            "OpenRouter (free models)",
            OpenAi,
            "https://openrouter.ai/api/v1",
            (false, true, true),
            "https://openrouter.ai/keys",
            ["", "", ""],
            "One key for many models; those whose names end in :free cost nothing. Fetch models to see the current free list.",
        ),
        preset(
            "groq",
            "Groq (free tier)",
            OpenAi,
            "https://api.groq.com/openai/v1",
            (false, true, true),
            "https://console.groq.com/keys",
            [
                "llama-3.3-70b-versatile",
                "meta-llama/llama-4-scout-17b-16e-instruct",
                "llama-3.1-8b-instant",
            ],
            "Very fast open models with a free tier and rate limits.",
        ),
        preset(
            "mistral",
            "Mistral (free tier)",
            OpenAi,
            "https://api.mistral.ai/v1",
            (false, true, true),
            "https://console.mistral.ai/api-keys",
            ["mistral-medium-latest", "pixtral-large-latest", "mistral-small-latest"],
            "European provider; the free Experiment plan has rate limits.",
        ),
        preset(
            "anthropic",
            "Anthropic Claude",
            Anthropic,
            "https://api.anthropic.com/v1",
            (false, true, false),
            "https://console.anthropic.com/settings/keys",
            ["claude-sonnet-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
            "Paid API. Haiku is quick and inexpensive for small jobs.",
        ),
        preset(
            "openai",
            "OpenAI",
            OpenAi,
            "https://api.openai.com/v1",
            (false, true, false),
            "https://platform.openai.com/api-keys",
            ["", "", ""],
            "Paid API. Fetch models to choose.",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, local: bool) -> Provider {
        Provider {
            id: id.into(),
            name: id.into(),
            kind: ProviderKind::OpenAi,
            base_url: "http://localhost:1/v1".into(),
            api_key: Some("secret".into()),
            local,
        }
    }

    #[test]
    fn keys_never_reach_the_ui_and_survive_edits() {
        let s = AiSettings {
            enabled: true,
            providers: vec![provider("a", true)],
            ..Default::default()
        };
        let public = s.public();
        let json = serde_json::to_string(&public).unwrap();
        assert!(!json.contains("secret"), "{json}");
        assert!(public.providers[0].has_key);
        // an edit without a new key keeps the stored one
        let kept = s.merge(public.clone()).unwrap();
        assert_eq!(kept.providers[0].api_key.as_deref(), Some("secret"));
        // an empty key clears it
        let mut cleared = public;
        cleared.providers[0].new_key = Some(String::new());
        assert_eq!(s.merge(cleared).unwrap().providers[0].api_key, None);
    }

    #[test]
    fn roles_fall_back_and_local_only_is_enforced() {
        let mut s = AiSettings {
            enabled: true,
            providers: vec![provider("cloud", false), provider("box", true)],
            chat: Some(ModelRef {
                provider: "cloud".into(),
                model: "big".into(),
            }),
            ..Default::default()
        };
        assert_eq!(s.resolve(Role::Fast).unwrap().1, "big");
        assert!(s.resolve(Role::Vision).is_err());
        s.local_only = true;
        assert!(s
            .resolve(Role::Chat)
            .unwrap_err()
            .contains("local models only"));
        s.chat = Some(ModelRef {
            provider: "box".into(),
            model: "small".into(),
        });
        assert_eq!(s.resolve(Role::Chat).unwrap().1, "small");
        s.enabled = false;
        assert!(s.resolve(Role::Chat).is_err());
    }

    #[test]
    fn rejects_bad_addresses_and_unknown_providers() {
        let s = AiSettings::default();
        let mut ui = AiSettings {
            providers: vec![provider("a", true)],
            ..Default::default()
        }
        .public();
        ui.providers[0].base_url = "localhost:11434".into();
        assert!(s.merge(ui.clone()).is_err());
        ui.providers[0].base_url = "http://localhost:11434/v1/".into();
        assert_eq!(
            s.merge(ui.clone()).unwrap().providers[0].base_url,
            "http://localhost:11434/v1"
        );
        ui.chat = Some(ModelRef {
            provider: "zzz".into(),
            model: "m".into(),
        });
        assert!(s.merge(ui).is_err());
    }

    #[test]
    fn presets_are_well_formed() {
        let p = presets();
        assert!(p.iter().any(|p| p.local && p.free));
        assert!(p.iter().filter(|p| !p.local && p.free).count() >= 3);
        for x in &p {
            assert!(x.base_url.starts_with("http"), "{}", x.id);
            assert_eq!(x.local, !x.needs_key, "{}", x.id);
        }
    }
}

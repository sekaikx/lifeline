//! # lifeline-ai
//!
//! An assistant that reads the planner's own numbers and the satellite
//! image of an area, and answers in plain language. It is model-agnostic:
//!
//! - **Local models** (Ollama, LM Studio, llama.cpp, vLLM) through the
//!   OpenAI-compatible API they all serve. Nothing leaves the machine; this
//!   is the setting for air-gapped and sensitive work.
//! - **Free cloud models**: the Google Gemini free tier, OpenRouter's free
//!   models, Groq and Mistral's free tiers.
//! - **Any other provider** with an OpenAI-compatible endpoint, and
//!   Anthropic's Messages API.
//!
//! Each of three roles gets its own model: `chat` for the analyst
//! conversation and reports, `vision` for reading satellite images, and
//! `fast` for small structured jobs such as turning a radio message into a
//! delivery request. A small fast model on the small jobs keeps them quick
//! and cheap; the larger models are kept for the work that needs them.
//!
//! The assistant is off until configured. Its answers are grounded: every
//! prompt carries the numbers the planner computed (plan, cover statistics,
//! hazards), the model is told to use only those, and structured answers
//! are parsed and range-checked before the UI sees them.

mod client;
mod grid;
mod settings;
mod tasks;

pub use client::{complete, list_models, Completion, Message, ModelInfo, Part};
pub use grid::{cell_center, overlay_grid, GRID};
pub use settings::{
    presets, AiSettings, ModelRef, Preset, Provider, ProviderKind, PublicProvider, PublicSettings,
    Role,
};
pub use tasks::{
    assess_area, chat, parse_request, AreaAssessment, CatalogueRef, Finding, ParsedItem,
    ParsedRequest, PlaceRef,
};

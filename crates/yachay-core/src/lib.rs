//! Núcleo de yachay: detección de hardware, base curada de modelos y
//! motor de recomendación.
//!
//! Este crate es una librería pura — sin I/O de consola ni red — para que
//! cualquier frontend (CLI, TUI, Tauri, o un nodo quipu consultándolo)
//! consuma la misma lógica.

mod hardware;
mod model;
mod recommend;

pub use hardware::{detect_hardware, DiskKind, HardwareProfile};
pub use model::{models, ModelSpec, Task};
pub use recommend::{recommend, CandidateVerdict, Recommendation};

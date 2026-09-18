//! Motor de recomendación: dado un perfil de hardware y una tarea,
//! elige el mejor modelo de la DB curada.
//!
//! Lógica del MVP (deliberadamente simple y explicable):
//! 1. Restricción dura: el modelo debe caber en la RAM usable.
//! 2. Entre los que caben y sirven para la tarea, gana el más capaz
//!    (más parámetros), con bonus si la tarea es su especialización.
//! 3. Todo veredicto guarda su razón — eso alimenta `--explain`.

use crate::hardware::HardwareProfile;
use crate::model::{models, ModelSpec, Task};

/// Bonus de capacidad si la tarea pedida es la especialización primaria
/// del modelo. Equivale a ~3B parámetros de ventaja: preferimos un
/// especialista 7B sobre un generalista 8B para su tarea.
const PRIMARY_TASK_BONUS: f64 = 3.0;

/// Base de tokens/seg por billón de parámetros en CPU moderna (Q4).
/// Heurística honesta: la inferencia en CPU está limitada por el ancho
/// de banda de memoria (~50-100 GB/s en laptops), y un 7B Q4 mueve
/// ~4.5GB por token. Es una estimación, no una medición.
const BASE_TPS_PER_BILLION: f64 = 75.0;

/// Qué le pasó a un candidato evaluado.
#[derive(Debug, Clone)]
pub enum CandidateVerdict {
    /// Cabe y sirve para la tarea — quedó rankeado.
    Fits { score: f64 },
    /// Cabe pero no está especializado en la tarea (queda como fallback).
    FitsOtherTask,
    /// No cabe en la RAM usable.
    TooBig { deficit_gb: f64 },
}

/// Resultado completo de una recomendación.
#[derive(Debug)]
pub struct Recommendation {
    /// El modelo elegido (None si nada cabe — hardware muy limitado).
    pub model: Option<ModelSpec>,
    /// Por qué se eligió, en texto humano.
    pub reason: String,
    /// Siguientes mejores opciones rankeadas (máx. 3).
    pub alternatives: Vec<ModelSpec>,
    /// Veredicto por modelo evaluado — alimenta `--explain`.
    pub verdicts: Vec<(ModelSpec, CandidateVerdict)>,
    /// Estimación gruesa de tokens/segundo en este hardware.
    pub estimated_tps: Option<f64>,
}

/// Estima tokens/segundo muy aproximados para un modelo en este hardware.
/// Apple Silicon (aarch64 + unified memory) es ~1.6x más rápido que
/// x86 comparable por ancho de banda de memoria.
fn estimate_tps(hw: &HardwareProfile, params_b: f64) -> f64 {
    let arch_factor = if matches!(hw.arch.as_str(), "aarch64" | "arm64") {
        1.6
    } else {
        1.0
    };
    (BASE_TPS_PER_BILLION * arch_factor / params_b).round() / 10.0 * 10.0
}

/// Recomienda un modelo para `task` en el hardware `hw`.
pub fn recommend(hw: &HardwareProfile, task: Task) -> Recommendation {
    let db = models();
    let mut verdicts = Vec::with_capacity(db.len());
    let mut ranked: Vec<(f64, &ModelSpec)> = Vec::new();
    let mut fallback: Vec<&ModelSpec> = Vec::new();

    for model in &db {
        if model.ram_needed_gb > hw.usable_ram_gb {
            verdicts.push((
                model.clone(),
                CandidateVerdict::TooBig {
                    deficit_gb: model.ram_needed_gb - hw.usable_ram_gb,
                },
            ));
            continue;
        }
        if model.supports(task) {
            let mut score = model.params_b;
            if model.primary_task() == Some(task) {
                score += PRIMARY_TASK_BONUS;
            }
            ranked.push((score, model));
            verdicts.push((model.clone(), CandidateVerdict::Fits { score }));
        } else {
            fallback.push(model);
            verdicts.push((model.clone(), CandidateVerdict::FitsOtherTask));
        }
    }

    // Si nada sirve para la tarea pero algo cabe, recomendamos el
    // generalista más capaz que entre — mejor que no recomendar nada.
    if ranked.is_empty() {
        if let Some(best_fallback) = fallback.iter().max_by(|a, b| {
            a.params_b
                .partial_cmp(&b.params_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            ranked.push((best_fallback.params_b, best_fallback));
        }
    }

    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let (model, alternatives, reason) = match ranked.split_first() {
        Some(((_, best), rest)) => {
            let reason = format!(
                "es el modelo más capaz para '{}' que cabe en tu RAM usable ({:.1} GB de {:.1} GB disponibles){}",
                task,
                best.ram_needed_gb,
                hw.usable_ram_gb,
                if best.primary_task() == Some(task) {
                    " y está especializado en esta tarea"
                } else {
                    ""
                }
            );
            let alts = rest.iter().take(3).map(|(_, m)| (*m).clone()).collect();
            (Some((*best).clone()), alts, reason)
        }
        None => (
            None,
            Vec::new(),
            format!(
                "ningún modelo de la DB cabe en tu RAM usable ({:.1} GB) — hardware muy limitado",
                hw.usable_ram_gb
            ),
        ),
    };

    let estimated_tps = model.as_ref().map(|m| estimate_tps(hw, m.params_b));

    Recommendation {
        model,
        reason,
        alternatives,
        verdicts,
        estimated_tps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw_with_ram(gb: f64) -> HardwareProfile {
        HardwareProfile {
            os: "test".into(),
            arch: "x86_64".into(),
            cpu_brand: "test cpu".into(),
            cpu_cores: 8,
            total_ram_gb: gb,
            usable_ram_gb: gb * 0.7,
            gpu_vram_gb: None,
            disk: crate::DiskKind::Unknown,
        }
    }

    #[test]
    fn recommends_a_coder_for_code() {
        let rec = recommend(&hw_with_ram(16.0), Task::Code);
        let model = rec.model.expect("debería recomendar algo con 16GB");
        assert!(
            model.primary_task() == Some(Task::Code),
            "para code debería elegir un especialista, eligió {}",
            model.name
        );
        // Con 16GB (11.2 usable) el mejor coder es el 14B (11.5GB no entra) → 7B.
        assert_eq!(model.ollama_tag, "qwen2.5-coder:7b");
    }

    #[test]
    fn respects_hard_ram_constraint() {
        let rec = recommend(&hw_with_ram(8.0), Task::Code);
        let model = rec.model.unwrap();
        assert!(model.ram_needed_gb <= 8.0 * 0.7);
    }

    #[test]
    fn picks_biggest_fitting_model() {
        // Con 64GB usable ~44.8 → cabe hasta Llama 70B para chat.
        let rec = recommend(&hw_with_ram(64.0), Task::Chat);
        let model = rec.model.unwrap();
        assert!(
            model.params_b >= 30.0,
            "con 64GB debería recomendar algo grande"
        );
    }

    #[test]
    fn tiny_ram_still_recommends_something() {
        let rec = recommend(&hw_with_ram(4.0), Task::Chat);
        let model = rec.model.expect("con 4GB debería caber un 1B");
        assert!(model.ram_needed_gb <= 4.0 * 0.7);
    }

    #[test]
    fn alternatives_are_also_fitting() {
        let rec = recommend(&hw_with_ram(16.0), Task::Code);
        for alt in &rec.alternatives {
            assert!(alt.supports(Task::Code));
            assert!(alt.ram_needed_gb <= 16.0 * 0.7);
        }
    }
}

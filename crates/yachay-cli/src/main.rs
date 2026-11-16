//! yachay — recomendador de modelos de IA locales.
//!
//! `yachay` sin args abre un wizard interactivo.
//! `yachay recommend --task code` da la recomendación directa.
//! `--explain` muestra por qué ganó y por qué se descartaron las demás.

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;
use std::io::IsTerminal;
use std::process::Command;
use std::str::FromStr;
use yachay_core::{
    detect_hardware, models, recommend, CandidateVerdict, HardwareProfile, ModelSpec,
    Recommendation, Task,
};

/// Argumentos de línea de comandos.
#[derive(Parser)]
#[command(
    name = "yachay",
    version,
    about = "Recomendador de modelos de IA locales según tu hardware y tu tarea"
)]
struct Cli {
    /// Subcomando; sin él se abre el wizard interactivo.
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcomandos disponibles.
#[derive(Subcommand)]
enum Commands {
    /// Recomienda un modelo para una tarea.
    Recommend {
        /// Tarea: code, chat, summarize, rag.
        #[arg(short, long)]
        task: Option<String>,
        /// Explica por qué ganó este modelo y por qué perdieron las demás.
        #[arg(short, long)]
        explain: bool,
    },
    /// Lista la base curada de modelos.
    Models,
}

/// Punto de entrada: detecta hardware una sola vez y despacha el subcomando.
///
/// Sin argumentos abre el wizard interactivo. Las tres ramas respetan la
/// cancelación (Esc) saliendo con código 0 en vez de imprimir un error.
fn main() -> Result<()> {
    let cli = Cli::parse();

    let hw = detect_hardware();
    match cli.command {
        Some(Commands::Recommend { task, explain }) => {
            let task = match task {
                Some(t) => Task::from_str(&t).map_err(anyhow::Error::msg)?,
                None => match ask_task()? {
                    Some(t) => t,
                    None => return Ok(()),
                },
            };
            run_recommend(&hw, task, explain);
        }
        Some(Commands::Models) => list_models(&hw),
        None => {
            // Sin args: wizard interactivo para no asustar a nadie.
            let Some(task) = ask_task()? else {
                return Ok(());
            };
            run_recommend(&hw, task, false);
        }
    }
    Ok(())
}

/// Pregunta qué tarea hará el usuario.
///
/// Devuelve `None` si cancela, para que quien llama decida salir sin ruido.
fn ask_task() -> Result<Option<Task>> {
    let options: Vec<String> = Task::ALL.iter().map(|t| t.label().to_string()).collect();
    let picked = match inquire::Select::new("¿Qué quieres hacer con el modelo?", options).prompt()
    {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    Ok(Task::ALL
        .iter()
        .position(|t| t.label() == picked)
        .map(|i| Task::ALL[i]))
}

/// Imprime la línea de contexto con el hardware detectado.
fn print_hardware(hw: &HardwareProfile) {
    println!(
        "{} {}",
        style("hardware:").dim(),
        style(format!(
            "{} · {} · {} cores · {:.0} GB RAM ({:.1} GB usable)",
            hw.os, hw.arch, hw.cpu_cores, hw.total_ram_gb, hw.usable_ram_gb
        ))
        .dim()
    );
    println!();
}

/// Muestra la recomendación completa y, si la salida es una terminal
/// interactiva, ofrece descargar el modelo o una alternativa.
fn run_recommend(hw: &HardwareProfile, task: Task, explain: bool) {
    print_hardware(hw);
    let rec = recommend(hw, task);

    let Some(model) = &rec.model else {
        println!("{} {}", style("✗").red(), rec.reason);
        return;
    };

    println!(
        "{} {}",
        style("Recomendación:").bold(),
        style(&model.name).green().bold()
    );
    println!("  {} {}", style("→").dim(), rec.reason);
    if let Some(tps) = rec.estimated_tps {
        println!(
            "  {} ~{:.0} tokens/seg estimados en tu hardware (aprox.)",
            style("→").dim(),
            tps
        );
    }
    println!("  {} {}", style("→").dim(), model.notes);
    println!();
    println!("Para correrlo:");
    println!(
        "  {}",
        style(format!("ollama run {}", model.ollama_tag))
            .cyan()
            .bold()
    );
    println!();

    if !rec.alternatives.is_empty() {
        println!("{}", style("Alternativas que también caben:").bold());
        for alt in &rec.alternatives {
            println!(
                "  · {} ({:.1}B — {:.1} GB RAM)",
                alt.name, alt.params_b, alt.ram_needed_gb
            );
        }
        println!();
    }

    if explain {
        print_explain(hw, task, &rec.verdicts);
    }

    // Si nos llamaron desde un pipe o un script, no bloqueamos esperando
    // una respuesta: solo imprimimos el comando y salimos.
    if std::io::stdin().is_terminal() {
        if let Err(err) = offer_download(&rec) {
            eprintln!("{} {err}", style("✗").red());
        }
    }
}

/// Pregunta si descargar el modelo elegido (o una alternativa) y delega
/// la descarga a Ollama.
///
/// No implementamos un downloader propio a propósito: `ollama pull` ya trae
/// barra de progreso, reanudación y verificación de checksum.
fn offer_download(rec: &Recommendation) -> Result<()> {
    const CANCEL: &str = "No, solo el comando";

    let candidates = download_candidates(rec);
    let mut options: Vec<String> = candidates.iter().map(option_label).collect();
    options.push(CANCEL.to_string());

    let picked = match inquire::Select::new("¿Descargo alguno ahora?", options).prompt() {
        Ok(p) => p,
        Err(_) => return Ok(()), // Esc: cerrar sin ruido
    };
    match candidate_for(&candidates, &picked) {
        Some(model) => pull(model),
        None => Ok(()),
    }
}

/// Los modelos descargables de una recomendación: el elegido primero y
/// luego las alternativas que también caben.
fn download_candidates(rec: &Recommendation) -> Vec<ModelSpec> {
    let mut candidates: Vec<ModelSpec> = rec.model.iter().cloned().collect();
    candidates.extend(rec.alternatives.iter().cloned());
    candidates
}

/// Traduce el texto que eligió el usuario al modelo correspondiente.
/// Devuelve `None` cuando eligió cancelar.
fn candidate_for<'a>(candidates: &'a [ModelSpec], picked: &str) -> Option<&'a ModelSpec> {
    candidates.iter().find(|m| option_label(m) == picked)
}

/// Texto de cada opción del selector, con el peso en RAM a la vista.
fn option_label(model: &ModelSpec) -> String {
    format!("{} ({:.1} GB de RAM)", model.name, model.ram_needed_gb)
}

/// Corre `ollama pull <tag>` heredando la terminal para que el usuario vea
/// el progreso real de la descarga.
fn pull(model: &ModelSpec) -> Result<()> {
    if !ollama_available() {
        println!();
        println!(
            "{} no encontré `ollama` en tu PATH — no puedo descargar.",
            style("✗").red()
        );
        println!("  Instálalo desde https://ollama.com/download y vuelve a correr yachay.");
        return Ok(());
    }

    println!();
    let status = Command::new("ollama")
        .args(["pull", &model.ollama_tag])
        .status()?;

    if status.success() {
        println!();
        println!("{} {} descargado.", style("✓").green(), model.name);
        println!("Puedes usarlo con:");
        println!(
            "  {}",
            style(format!("ollama run {}", model.ollama_tag))
                .cyan()
                .bold()
        );
    } else {
        anyhow::bail!(
            "`ollama pull {}` falló ({}) — revisa el mensaje de arriba",
            model.ollama_tag,
            status
        );
    }
    Ok(())
}

/// ¿Hay un binario de Ollama ejecutable en el PATH?
fn ollama_available() -> bool {
    Command::new("ollama").arg("--version").output().is_ok()
}

/// Explica, modelo por modelo, por qué cabe o no cabe en esta máquina.
fn print_explain(hw: &HardwareProfile, task: Task, verdicts: &[(ModelSpec, CandidateVerdict)]) {
    println!("{}", style("¿Por qué este modelo?").bold());
    println!(
        "  RAM usable: {:.1} GB (de {:.0} GB totales, dejando espacio al SO)\n",
        hw.usable_ram_gb, hw.total_ram_gb
    );
    for (model, verdict) in verdicts {
        match verdict {
            CandidateVerdict::Fits { .. } if model.supports(task) => {
                println!(
                    "  {} {} — cabe ({:.1} GB) y sirve para {}",
                    style("✓").green(),
                    model.name,
                    model.ram_needed_gb,
                    task
                );
            }
            CandidateVerdict::FitsOtherTask => {
                println!(
                    "  {} {} — cabe pero no está hecho para {}",
                    style("·").dim(),
                    style(&model.name).dim(),
                    task
                );
            }
            CandidateVerdict::TooBig { deficit_gb } => {
                println!(
                    "  {} {} — necesita {:.1} GB más de los que puedes ofrecer",
                    style("✗").red(),
                    style(&model.name).dim(),
                    deficit_gb
                );
            }
            _ => {}
        }
    }
}

/// Tabla completa de la DB con ✓/✗ según si cada modelo cabe en la máquina.
fn list_models(hw: &HardwareProfile) {
    print_hardware(hw);
    println!("{}", style("Base curada de modelos:").bold());
    for m in models() {
        let fits = if m.ram_needed_gb <= hw.usable_ram_gb {
            style("✓").green()
        } else {
            style("✗").red()
        };
        let tasks: Vec<String> = m.tasks.iter().map(|t| t.to_string()).collect();
        println!(
            "  {} {:<24} {:>5.1}B  {:>5.1} GB  [{}]",
            fits,
            m.name,
            m.params_b,
            m.ram_needed_gb,
            tasks.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, tag: &str) -> ModelSpec {
        ModelSpec {
            name: name.into(),
            ollama_tag: tag.into(),
            params_b: 7.0,
            ram_needed_gb: 6.8,
            tasks: vec![Task::Code],
            notes: String::new(),
        }
    }

    fn rec(model: ModelSpec, alternatives: Vec<ModelSpec>) -> Recommendation {
        Recommendation {
            model: Some(model),
            reason: String::new(),
            alternatives,
            verdicts: Vec::new(),
            estimated_tps: None,
        }
    }

    #[test]
    fn candidate_for_maps_option_text_back_to_model() {
        let rec = rec(model("A", "a:7b"), vec![model("B", "b:7b")]);
        let candidates = download_candidates(&rec);
        assert_eq!(candidates.len(), 2);

        let picked = option_label(&candidates[1]);
        let found = candidate_for(&candidates, &picked).unwrap();
        assert_eq!(found.ollama_tag, "b:7b");
    }

    #[test]
    fn candidate_for_returns_none_on_cancel() {
        let rec = rec(model("A", "a:7b"), Vec::new());
        let candidates = download_candidates(&rec);
        assert!(candidate_for(&candidates, "No, solo el comando").is_none());
    }
}

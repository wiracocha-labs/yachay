//! yachay — recomendador de modelos de IA locales.
//!
//! `yachay` sin args abre un wizard interactivo.
//! `yachay recommend --task code` da la recomendación directa.
//! `--explain` muestra por qué ganó y por qué se descartaron las demás.

use anyhow::Result;
use clap::{Parser, Subcommand};
use console::style;
use std::str::FromStr;
use yachay_core::{
    detect_hardware, models, recommend, CandidateVerdict, HardwareProfile, ModelSpec, Task,
};

#[derive(Parser)]
#[command(
    name = "yachay",
    version,
    about = "Recomendador de modelos de IA locales según tu hardware y tu tarea"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

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

fn main() -> Result<()> {
    let cli = Cli::parse();

    let hw = detect_hardware();
    match cli.command {
        Some(Commands::Recommend { task, explain }) => {
            let task = match task {
                Some(t) => Task::from_str(&t).map_err(anyhow::Error::msg)?,
                None => ask_task()?,
            };
            run_recommend(&hw, task, explain);
        }
        Some(Commands::Models) => list_models(&hw),
        None => {
            // Sin args: wizard interactivo para no asustar a nadie.
            let task = ask_task()?;
            run_recommend(&hw, task, false);
        }
    }
    Ok(())
}

fn ask_task() -> Result<Task> {
    let options: Vec<String> = Task::ALL.iter().map(|t| t.label().to_string()).collect();
    let picked = inquire::Select::new("¿Qué quieres hacer con el modelo?", options).prompt()?;
    let idx = Task::ALL
        .iter()
        .position(|t| t.label() == picked)
        .unwrap_or(0);
    Ok(Task::ALL[idx])
}

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
}

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

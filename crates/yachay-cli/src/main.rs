//! yachay — recomendador de modelos de IA locales.
//!
//! `yachay` sin args abre un wizard interactivo.
//! `yachay recommend --task code` da la recomendación directa.
//! `--explain` muestra por qué ganó y por qué se descartaron las demás.
//!
//! La interfaz está en inglés; los comentarios de código, en español.

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
    about = "Find the best open-source local AI model for your hardware and task"
)]
struct Cli {
    /// Subcomando; sin él se abre el wizard interactivo.
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcomandos disponibles.
#[derive(Subcommand)]
enum Commands {
    /// Recommend a model for a task.
    Recommend {
        /// Task: code, chat, summarize, rag.
        #[arg(short, long)]
        task: Option<String>,
        /// Explain why this model won and why the others lost.
        #[arg(short, long)]
        explain: bool,
    },
    /// List the curated model database.
    Models,
    /// Analyze your hardware and pick what to download.
    Run,
    /// List the models you already downloaded (delegates to `ollama list`).
    Ls,
}

/// Punto de entrada: detecta hardware una sola vez y despacha el subcomando.
///
/// Sin argumentos abre el wizard interactivo. Las ramas interactivas respetan
/// la cancelación (Esc) saliendo con código 0 en vez de imprimir un error.
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
            print_hardware(&hw);
            run_recommend(&hw, task, explain);
        }
        Some(Commands::Models) => list_models(&hw),
        Some(Commands::Ls) => list_local()?,
        // `run` es el flujo interactivo; sin args se comporta igual.
        None | Some(Commands::Run) => {
            print_intro();
            print_hardware(&hw);
            let Some(task) = ask_task()? else {
                return Ok(());
            };
            run_recommend(&hw, task, false);
        }
    }
    Ok(())
}

/// Explica, antes de preguntar, por qué la tarea cambia la recomendación.
///
/// Sin esto, la primera pantalla es una pregunta sin contexto: el usuario no
/// sabe que "code" y "chat" llevan a bases de modelos distintas.
fn print_intro() {
    println!(
        "{}",
        style("yachay — find the right open-source model for your machine.").bold()
    );
    println!();
    println!("  We ask what you'll use the model for, because the best choice");
    println!("  depends on it: a code-specialized model beats a general one at");
    println!("  programming, and the other way around for chat.");
    println!();
}

/// Pregunta qué tarea hará el usuario.
///
/// Devuelve `None` si cancela, para que quien llama decida salir sin ruido.
fn ask_task() -> Result<Option<Task>> {
    let options: Vec<String> = Task::ALL.iter().map(|t| t.label().to_string()).collect();
    let picked = match inquire::Select::new("What will you use the model for?", options).prompt() {
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
    let rec = recommend(hw, task);

    let Some(model) = &rec.model else {
        println!("{} {}", style("✗").red(), rec.reason);
        return;
    };

    println!(
        "{} {}",
        style("Recommendation:").bold(),
        style(&model.name).green().bold()
    );
    println!("  {} {}", style("→").dim(), rec.reason);
    if let Some(tps) = rec.estimated_tps {
        println!(
            "  {} ~{:.0} tokens/sec estimated on your hardware (approximate)",
            style("→").dim(),
            tps
        );
    }
    println!("  {} {}", style("→").dim(), model.notes);
    println!();
    println!("To run it:");
    println!(
        "  {}",
        style(format!("ollama run {}", model.ollama_tag))
            .cyan()
            .bold()
    );
    println!();

    if !rec.alternatives.is_empty() {
        println!("{}", style("Alternatives that also fit:").bold());
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

/// Opción del selector que significa "no descargues nada".
const CANCEL_OPTION: &str = "No, cancel";

/// Pregunta si descargar el modelo elegido (o una alternativa) y delega
/// la descarga a Ollama.
///
/// No implementamos un downloader propio a propósito: `ollama pull` ya trae
/// barra de progreso, reanudación y verificación de checksum.
fn offer_download(rec: &Recommendation) -> Result<()> {
    let candidates = download_candidates(rec);
    let mut options: Vec<String> = candidates.iter().map(option_label).collect();
    options.push(CANCEL_OPTION.to_string());

    let picked = match inquire::Select::new("Download one now?", options).prompt() {
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
    format!("{} ({:.1} GB RAM)", model.name, model.ram_needed_gb)
}

/// Página oficial desde donde se instala Ollama en cualquier plataforma.
const OLLAMA_URL: &str = "https://ollama.com/download";

/// Instalador oficial: cubre Linux y macOS. Windows no lo tiene.
const OLLAMA_INSTALL_CMD: &str = "curl -fsSL https://ollama.com/install.sh | sh";

/// Lista los modelos ya descargados en esta máquina.
///
/// Delega en `ollama list` sin parsear la salida: es una tabla ya legible y
/// parsearla se rompería con cualquier cambio de formato de Ollama.
fn list_local() -> Result<()> {
    if !ensure_ollama()? {
        return Ok(());
    }
    let status = Command::new("ollama").arg("list").status()?;
    if !status.success() {
        anyhow::bail!("`ollama list` failed ({status})");
    }
    Ok(())
}

/// Asegura que `ollama` esté disponible. Si falta, avisa, muestra la página
/// oficial y ofrece instalarlo sin salir de la terminal.
///
/// Devuelve `false` si el usuario prefiere instalarlo por su cuenta, para que
/// quien llama termine con un mensaje útil en vez de un error.
fn ensure_ollama() -> Result<bool> {
    if ollama_available() {
        return Ok(true);
    }

    println!();
    println!(
        "{} could not find `ollama` — it's needed to download models.",
        style("✗").red()
    );
    println!("  Official page: {}", style(OLLAMA_URL).cyan());
    println!();

    // Windows no tiene script oficial: solo podemos guiar al instalador.
    if cfg!(windows) {
        println!("  Download `OllamaSetup.exe` from that page, then run yachay again.");
        return Ok(false);
    }

    let install = inquire::Confirm::new("Install Ollama now to continue?")
        .with_default(false)
        .prompt()
        .unwrap_or(false);
    if !install {
        return Ok(false);
    }

    // Mostramos el comando exacto antes de ejecutarlo: el usuario debe poder
    // auditarlo. No hay input del usuario en la cadena, así que no hay inyección.
    println!();
    println!("  {}", style(OLLAMA_INSTALL_CMD).dim());
    if cfg!(target_os = "linux") {
        // En Linux el script instala en /usr/local/bin, crea el usuario
        // `ollama` y registra un servicio systemd: todo eso requiere root.
        println!(
            "  {}",
            style("On Linux this uses `sudo` and may ask for your password.").dim()
        );
    }
    println!();
    let status = Command::new("sh")
        .args(["-c", OLLAMA_INSTALL_CMD])
        .status()?;
    if !status.success() {
        anyhow::bail!(
            "Ollama installation did not finish ({status}) — you can run it yourself:\n  {}",
            OLLAMA_INSTALL_CMD
        );
    }

    // En macOS el instalador abre la app en segundo plano, así que el daemon
    // puede tardar unos segundos en responder.
    if !wait_for_daemon() {
        anyhow::bail!("Ollama installed but is not responding — open it and try again");
    }
    println!("{} Ollama ready.", style("✓").green());
    Ok(true)
}

/// Espera hasta 5 segundos a que el daemon de Ollama conteste.
fn wait_for_daemon() -> bool {
    for _ in 0..20 {
        let up = Command::new("ollama")
            .arg("list")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if up {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    false
}

/// Corre `ollama pull <tag>` heredando la terminal para que el usuario vea
/// el progreso real de la descarga.
fn pull(model: &ModelSpec) -> Result<()> {
    if !ensure_ollama()? {
        println!();
        println!("Once you have Ollama, run:");
        println!(
            "  {}",
            style(format!("ollama pull {}", model.ollama_tag)).cyan()
        );
        return Ok(());
    }

    println!();
    let status = Command::new("ollama")
        .args(["pull", &model.ollama_tag])
        .status()?;

    if status.success() {
        println!();
        println!("{} {} downloaded.", style("✓").green(), model.name);
        println!("Run it with:");
        println!(
            "  {}",
            style(format!("ollama run {}", model.ollama_tag))
                .cyan()
                .bold()
        );
    } else {
        anyhow::bail!(
            "`ollama pull {}` failed ({}) — see the message above",
            model.ollama_tag,
            status
        );
    }
    Ok(())
}

/// ¿Hay un binario de Ollama que responde?
///
/// No basta con que exista: un `ollama` roto también está en el PATH y
/// entonces conviene ofrecer la reinstalación.
fn ollama_available() -> bool {
    Command::new("ollama")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Explica, modelo por modelo, por qué cabe o no cabe en esta máquina.
fn print_explain(hw: &HardwareProfile, task: Task, verdicts: &[(ModelSpec, CandidateVerdict)]) {
    println!("{}", style("Why this model?").bold());
    println!(
        "  Usable RAM: {:.1} GB (out of {:.0} GB total, leaving room for the OS)\n",
        hw.usable_ram_gb, hw.total_ram_gb
    );
    for (model, verdict) in verdicts {
        match verdict {
            CandidateVerdict::Fits { .. } if model.supports(task) => {
                println!(
                    "  {} {} — fits ({:.1} GB) and is built for {}",
                    style("✓").green(),
                    model.name,
                    model.ram_needed_gb,
                    task
                );
            }
            CandidateVerdict::FitsOtherTask => {
                println!(
                    "  {} {} — fits, but isn't made for {}",
                    style("·").dim(),
                    style(&model.name).dim(),
                    task
                );
            }
            CandidateVerdict::TooBig { deficit_gb } => {
                println!(
                    "  {} {} — needs {:.1} GB more than you can offer",
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
    println!("{}", style("Curated model database:").bold());
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
        assert!(candidate_for(&candidates, CANCEL_OPTION).is_none());
    }
}

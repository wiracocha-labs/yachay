//! Tipos de tarea y la base curada de modelos.
//!
//! La DB vive en `data/models.json` (embebida en el binario con
//! `include_str!`) para que curarla sea editar datos, no código.

use serde::Deserialize;
use std::fmt;
use std::str::FromStr;

/// Tipo de trabajo que el usuario quiere hacer con el modelo.
/// Determina qué modelos especializados convienen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Task {
    /// Programación: autocompletado, explicar código, refactors.
    Code,
    /// Conversación general / asistente.
    Chat,
    /// Resumir documentos largos.
    Summarize,
    /// Retrieval-augmented generation sobre documentos propios.
    Rag,
}

impl Task {
    pub const ALL: [Task; 4] = [Task::Code, Task::Chat, Task::Summarize, Task::Rag];

    /// Nombre para mostrar en la UI.
    pub fn label(&self) -> &'static str {
        match self {
            Task::Code => "código",
            Task::Chat => "chat / asistente",
            Task::Summarize => "resumir documentos",
            Task::Rag => "RAG sobre documentos propios",
        }
    }
}

impl fmt::Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Task::Code => "code",
            Task::Chat => "chat",
            Task::Summarize => "summarize",
            Task::Rag => "rag",
        })
    }
}

impl FromStr for Task {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "code" => Ok(Task::Code),
            "chat" => Ok(Task::Chat),
            "summarize" | "resumen" => Ok(Task::Summarize),
            "rag" => Ok(Task::Rag),
            other => Err(format!(
                "tarea desconocida '{other}' — opciones: code, chat, summarize, rag"
            )),
        }
    }
}

/// Un modelo open-source con requisitos de hardware verificados.
/// Los campos se cargan desde `data/models.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelSpec {
    /// Nombre para mostrar, ej. "Qwen2.5-Coder 7B".
    pub name: String,
    /// Tag para `ollama run <tag>`.
    pub ollama_tag: String,
    /// Parámetros en miles de millones.
    pub params_b: f64,
    /// RAM necesaria en GB para correrlo cómodo en Q4
    /// (pesos + contexto + overhead del runtime).
    pub ram_needed_gb: f64,
    /// Tareas para las que sirve; la primera es su especialización primaria.
    pub tasks: Vec<Task>,
    /// Nota curada — por qué está en la DB.
    pub notes: String,
}

impl ModelSpec {
    /// Tarea principal del modelo (la primera de la lista).
    pub fn primary_task(&self) -> Option<Task> {
        self.tasks.first().copied()
    }

    /// ¿Sirve para esta tarea?
    pub fn supports(&self, task: Task) -> bool {
        self.tasks.contains(&task)
    }
}

/// La base curada de modelos, parseada desde el JSON embebido.
pub fn models() -> Vec<ModelSpec> {
    serde_json::from_str(include_str!("../data/models.json"))
        .expect("models.json debe ser válido — revisar la DB")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_loads_and_is_sane() {
        let db = models();
        assert!(db.len() >= 10, "la DB debería tener 10+ modelos");
        for m in &db {
            assert!(!m.tasks.is_empty(), "{} sin tareas", m.name);
            assert!(m.ram_needed_gb > 0.0, "{} sin RAM definida", m.name);
            assert!(!m.ollama_tag.is_empty(), "{} sin tag de ollama", m.name);
        }
    }

    #[test]
    fn task_parses_from_str() {
        assert_eq!("code".parse::<Task>().unwrap(), Task::Code);
        assert_eq!("RAG".parse::<Task>().unwrap(), Task::Rag);
        assert!("xyz".parse::<Task>().is_err());
    }
}

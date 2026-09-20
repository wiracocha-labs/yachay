# Yachay

*Yachay* means "knowledge" in Quechua.

A local AI model recommender built in Rust, part of
[Wiracocha Labs](https://github.com/wiracocha-labs).

**Status:** Phase 1 — MVP CLI `Released` · [v0.1.0](https://github.com/wiracocha-labs/yachay/releases/tag/v0.1.0)

---

## Install (no Rust required)

Pre-compiled binaries for macOS (Apple Silicon + Intel), Linux (x86 + ARM)
and Windows are published on
[GitHub Releases](https://github.com/wiracocha-labs/yachay/releases/latest).

**macOS / Linux:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/wiracocha-labs/yachay/releases/latest/download/yachay-cli-installer.sh | sh
```

**Windows (PowerShell):**

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/wiracocha-labs/yachay/releases/latest/download/yachay-cli-installer.ps1 | iex"
```

Or download the archive for your platform directly from the release page.

## Quickstart

```bash
# Recomendación directa para una tarea
yachay recommend --task code

# Con explicación de por qué ganó y qué se descartó
yachay recommend --task code --explain

# Wizard interactivo (sin args)
yachay

# Ver la base curada y qué cabe en tu hardware
yachay models
```

To build from source instead:

```bash
cargo install --git https://github.com/wiracocha-labs/yachay yachay-cli
```

---

## What it does

Yachay analyzes your hardware and your actual task, then tells you exactly
which open-source local AI model to use — and how to run it. No guessing,
no downloading models that don't fit your RAM, no paying for cloud inference
you don't need.

---

## The problem it solves

Running local AI today means:

- Searching HuggingFace or Ollama without knowing what fits your hardware.
- Downloading a model that turns out to be too large for your RAM.
- Not knowing that a smaller, specialized model would serve your task better
  than the large one you just downloaded.
- Wasting hours on setup instead of on work.

Yachay solves this with one command.

---

## Target users

**First:** developers running local AI on older or modest hardware (5-13 year
old laptops, machines without a dedicated GPU, Raspberry Pi).

**Eventually:** any person who wants to run AI locally without technical
knowledge — but that requires a GUI, which is out of scope for the initial
CLI version.

---

## Scope

### In scope (MVP)
- CLI in Rust that detects available hardware (RAM, CPU cores, presence of
  GPU/VRAM, disk type — NVMe vs SATA vs HDD)
- Curated database of open-source models with verified hardware requirements
  (Llama 3.2, Qwen, Phi-3-mini, Gemma, Mistral, and others)
- Recommendation logic based on:
  - Available RAM (hard constraint — model must fit)
  - Task type (text/code, summarization, RAG, image — selects specialized
    models over general ones)
  - Disk type (NVMe enables MoE expert streaming for larger models)
  - Single user vs. multi-user (affects latency tolerance)
- Output: exact model name + download command (Ollama or direct GGUF link)
  + estimated tokens/second on detected hardware
- Optional flag: `--explain` prints why this model was recommended over
  alternatives

### Out of scope (for now)
- GUI or web interface
- Model training or fine-tuning
- Cloud model recommendations (local-only)
- Auto-download or auto-install of models
- Multi-user server mode

---

## Why Rust

- Runs natively on Linux, macOS, and Windows without a runtime.
- Fast hardware detection without dependencies.
- Consistent with the rest of the Wiracocha Labs stack.
- Cross-compiles easily for Raspberry Pi and other ARM devices.

---

## Roadmap

No dates — verifiable milestones.

### Phase 0 — Scope definition `Done`
- Define recommendation logic and hardware detection approach.
- Curate initial model database (10-15 models with verified requirements).

**Exit criterion:** this README reviewed and confirmed accurate.

### Phase 1 — MVP CLI `Done` (v0.1.0)
- Hardware detection working on Linux and macOS.
- Recommendation logic for RAM + task type (core constraints).
- Initial curated model database.
- `yachay recommend` command returns a usable result.

**Exit criterion:** someone on a 5-year-old laptop with 16GB RAM runs
`yachay recommend --task code` and gets a correct, installable recommendation
without reading any documentation.

### Phase 2 — Expanded coverage `Planned`
- Disk type detection (NVMe vs SATA) enabling MoE streaming recommendations.
- GPU/VRAM detection for machines with a dedicated GPU.
- `--explain` flag with reasoning output.
- Model database expanded to 30+ models.
- Windows support.

**Exit criterion:** Yachay recommends correctly across at least 10 different
real hardware configurations, verified by external testers.

### Phase 3 — Integration with Wiracocha Labs ecosystem `Planned`
- Yachay used as the entry point for Chaka's research (recommends the base
  model before delta compression experiments).
- Optional integration with quipu-ipfs: nodes announce their hardware profile
  so the network knows what models each node can run.

**Exit criterion:** Chaka and quipu-ipfs can query Yachay programmatically,
not just via CLI.

---

## Relation to other Wiracocha Labs projects

| Project | Relation |
|---|---|
| **Chaka** | Yachay recommends the base model; Chaka researches how to compress and sync its updates between nodes |
| **quipu-ipfs** | Future: nodes expose hardware profiles; Yachay logic runs at the network level to match tasks to capable nodes |
| **Chasqui** | Independent — different product domain |

---

## License

AGPL-3.0 — see [LICENSE](./LICENSE)

---

## Part of Wiracocha Labs

Wiracocha Labs is open research, built in public, from Latin America.
[github.com/wiracocha-labs](https://github.com/wiracocha-labs)

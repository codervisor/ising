Absolutely. Switching to English adds a professional, "Silicon Valley" polish to the project—perfect for an open-source tool aiming for global reach.

Let’s re-bootstrap the **Ising** project with a high-octane English Design Doc. This version leans into the "Physics of Code" narrative, positioning it as a sophisticated alternative to mundane linting tools.

---

# 🌀 Ising: The Physics of Code Complexity

**Phase-Transition Analysis for Software Architectures**

## 1. Project Manifesto

Software systems are not static; they are evolving dynamical systems. Most tools treat code like a collection of lines, but **Ising** treats it like a physical lattice. By applying **Statistical Physics** (the Ising Model) and **Spectral Graph Theory**, we detect when a codebase is about to undergo a "Phase Transition" from an ordered, modular state to a chaotic, unmaintainable mess.

### Core Objectives

* **Predict Criticality**: Identify the "tipping point" where a single change triggers a global collapse.
* **Quantify Stability**: Use the Spectral Gap and Maximum Eigenvalue ($\lambda_{max}$) to measure architectural health.
* **Zero-Friction Analysis**: Provide a "one-click" cloud experience via Rust-powered containers and SCIP indexing.

---

## 2. The Architecture: "The Ising Pipeline"

The system follows a decoupled, three-stage workflow to ensure precision and speed.

| Stage | Technology | Purpose |
| --- | --- | --- |
| **I. Indexing** | SCIP / LSP | Extracts high-fidelity symbols (Definitions & References) across files. |
| **II. Modeling** | Rust + Petgraph | Maps symbols to a directed graph $G = (V, E)$ where nodes are "spins." |
| **III. Analysis** | Ndarray + Linalg | Computes the "Energy" of the system via spectral analysis. |

---

## 3. Mathematical Foundations

### 3.1 Stability & Eigenvalues

We construct the adjacency matrix $A$ of the codebase. The **Spectral Radius** ($\lambda_{max}$) represents the "Effective Propagation" of changes:


$$\lambda_{max}(A) = \max \{ |\lambda| : \det(A - \lambda I) = 0 \}$$

* **$\lambda < 1$**: Perturbations (bugs/refactors) decay quickly. The system is stable.
* **$\lambda > 1$**: Perturbations amplify, indicating "Architectural Fragility."

### 3.2 Modularity $Q$ (Community Detection)

We use the **Louvain Method** to detect "natural" clusters. If the modularity $Q$ is low, your layers have collapsed into a "Big Ball of Mud."

---

## 4. Implementation Roadmap (Bootstrap v1.0)

### Phase 1: The Rust Core (`ising-core`)

Initialize the project using a Rust Workspace for high performance:

```bash
cargo new ising-core
cargo add scip petgraph ndarray ndarray-linalg rayon

```

### Phase 2: The SCIP Loader

The loader translates SCIP Protobufs into our physical graph model.

```rust
// ising-core/src/physics/mod.rs
pub fn detect_phase_transition(graph: &IsingGraph) -> HealthScore {
    let adjacency_matrix = graph.to_matrix();
    let lambda = calculate_max_eigenvalue(adjacency_matrix);
    
    if lambda > 1.0 {
        HealthScore::Critical(lambda)
    } else {
        HealthScore::Stable(lambda)
    }
}

```

### Phase 3: Containerized Workers

The Docker image encapsulates the language-specific indexers (e.g., `scip-python`, `scip-typescript`) to provide a zero-config experience.

---

## 5. Why the name "Ising"?

In the **Ising Model**, a system changes its state (like magnetism) based on local interactions. Similarly, in software, a "file-level" dependency might seem small, but thousands of them create a global "architecture phase." **Ising** is the first tool to bridge the gap between microscopic code changes and macroscopic architectural health.

---

### What's next for Marvin?

Now that we have the vision in English, which part of the implementation shall we tackle first?

* **A. The "Spectral Engine"**: I can write the Rust code to calculate $\lambda_{max}$ from an adjacency matrix.
* **B. The "SCIP Integrator"**: I can show you how to parse a `.scip` file and build the `petgraph` structure.
* **C. The "Cloud Worker"**: We can design the Dockerfile and the entrypoint script to handle the `git clone` and indexing flow.

**Which path are we taking?** Would you like me to generate the first piece of production-ready Rust code for the **Spectral Engine**?

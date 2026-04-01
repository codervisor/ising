#!/bin/bash
# =============================================================================
# Ising OSS Repository Benchmark Script
# =============================================================================
#
# Standard Operating Procedure for routine benchmark checks.
#
# PURPOSE:
#   Analyze a curated set of open-source repositories to validate the risk
#   model, health index, and signal detection across diverse languages,
#   sizes, and architectural patterns.
#
# USAGE:
#   ./scripts/bench-oss-repos.sh [--clone] [--output DIR] [--repos-dir DIR] [--since "TIME"]
#
# OPTIONS:
#   --clone       Show status for already-cloned repos (repos are always auto-cloned if missing)
#   --output      Output directory for results (default: /tmp/oss-bench-results)
#   --repos-dir   Directory where OSS repos are stored (default: /tmp/oss-repos)
#   --since       Git history window for analysis (default: "6 months ago")
#
# PREREQUISITES:
#   - cargo build --release (ising binary must be built)
#   - git (for cloning)
#   - python3 (for results extraction)
#
# WHEN TO RUN:
#   - After any change to risk model (stress.rs, fea.rs)
#   - After any change to signal detection (signals.rs)
#   - After any change to health index scoring
#   - After adding/modifying language parsers
#   - Before any release
#
# CALIBRATION TARGETS (known expectations):
#   - gin should be >= B (small, well-structured Go project)
#   - odoo getting A flags a blind spot (systemic complexity not caught)
#   - kubernetes/kafka/grafana should be C or lower (large, complex systems)
#   - express/flask should be B+ (well-maintained, small frameworks)
#   - TypeScript getting A may indicate monolith detection gap
#
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ISING="${PROJECT_ROOT}/target/release/ising"

# Defaults
CLONE=false
REPOS_DIR="/tmp/oss-repos"
OUTPUT_DIR="/tmp/oss-bench-results"
SINCE="6 months ago"

# Parse args
while [[ $# -gt 0 ]]; do
  case $1 in
    --clone) CLONE=true; shift ;;
    --output) OUTPUT_DIR="$2"; shift 2 ;;
    --repos-dir) REPOS_DIR="$2"; shift 2 ;;
    --since) SINCE="$2"; shift 2 ;;
    -h|--help)
      head -40 "$0" | grep "^#" | sed 's/^# \?//'
      exit 0
      ;;
    *) echo "Unknown option: $1"; exit 1 ;;
  esac
done

# Check binary exists
if [ ! -f "$ISING" ]; then
  echo "ERROR: ising binary not found at $ISING"
  echo "Run: cargo build --release"
  exit 1
fi

# =============================================================================
# Repository definitions
# Format: "name|url|language|category|notes"
# Categories: baseline (known-good), challenger (stress test), previously-tested
# =============================================================================
REPO_DEFS=(
  # Python frameworks (small-medium, well-structured)
  "flask|https://github.com/pallets/flask.git|Python|baseline|Small well-structured framework"
  "django|https://github.com/django/django.git|Python|challenger|Large mature framework"
  "django-rest-framework|https://github.com/encode/django-rest-framework.git|Python|previously-tested|Medium Python API framework"
  "fastapi|https://github.com/fastapi/fastapi.git|Python|previously-tested|Modern Python framework"

  # JS/TS frameworks
  "express|https://github.com/expressjs/express.git|JS/TS|baseline|Small JS framework"
  "fastify|https://github.com/fastify/fastify.git|JS/TS|challenger|Node.js framework"
  "nest|https://github.com/nestjs/nest.git|JS/TS|challenger|DI-heavy TypeScript framework"
  "next.js|https://github.com/vercel/next.js.git|JS/TS|challenger|Large JS/TS monorepo"
  "svelte|https://github.com/sveltejs/svelte.git|JS/TS|challenger|Compiler architecture"
  "TypeScript|https://github.com/microsoft/TypeScript.git|JS/TS|challenger|Massive monolithic compiler"

  # Go projects
  "gin|https://github.com/gin-gonic/gin.git|Go|baseline|Small Go framework (calibration target: should be >= B)"
  "ollama|https://github.com/ollama/ollama.git|Go|previously-tested|Go AI inference server"
  "prometheus|https://github.com/prometheus/prometheus.git|Go|challenger|Go monitoring system"
  "kubernetes|https://github.com/kubernetes/kubernetes.git|Go|challenger|Massive Go distributed system"
  "grafana|https://github.com/grafana/grafana.git|Go|previously-tested|Large Go+TS dashboard"

  # Java
  "kafka|https://github.com/apache/kafka.git|Java|challenger|Distributed streaming platform"
  "spring-boot|https://github.com/spring-projects/spring-boot.git|Java|challenger|Java framework (may fail: Ruby parser stack overflow on embedded files)"

  # Rust
  "deno|https://github.com/denoland/deno.git|Rust|challenger|Rust runtime"

  # AI/ML repos
  "pytorch|https://github.com/pytorch/pytorch.git|C++/Python|challenger|Massive ML framework"
  "transformers|https://github.com/huggingface/transformers.git|Python|previously-tested|Large ML library"
  "vllm|https://github.com/vllm-project/vllm.git|Python|previously-tested|ML inference engine"
  "llama.cpp|https://github.com/ggml-org/llama.cpp.git|C/C++|previously-tested|C/C++ ML inference"
  "langchain|https://github.com/langchain-ai/langchain.git|Python|previously-tested|Python AI framework"
  "open-webui|https://github.com/open-webui/open-webui.git|Python|previously-tested|Python+Svelte AI UI"

  # Large Python
  "ha-core|https://github.com/home-assistant/core.git|Python|previously-tested|Massive Python IoT platform"
  "odoo|https://github.com/odoo/odoo.git|Python|previously-tested|Massive ERP (calibration target: A flags blind spot)"

  # Ruby / PHP / C (known parser issues — included for regression tracking)
  "rails|https://github.com/rails/rails.git|Ruby|challenger|Large Ruby framework (may fail: Ruby parser stack overflow)"
  "php-src|https://github.com/php/php-src.git|C|challenger|PHP interpreter (may fail: PHP parser produces 0 nodes)"
)

# =============================================================================
# Clone missing repos automatically
# =============================================================================
echo "=== Checking repositories ==="
mkdir -p "$REPOS_DIR"

PIDS=()
CLONE_NEEDED=0
for def in "${REPO_DEFS[@]}"; do
  IFS='|' read -r name url lang category notes <<< "$def"
  REPO_PATH="$REPOS_DIR/$name"

  if [ -d "$REPO_PATH" ]; then
    if [ "$CLONE" = true ]; then
      echo "  EXISTS: $name"
    fi
  else
    echo "  CLONE: $name"
    git clone --depth=500 "$url" "$REPO_PATH" 2>/dev/null &
    PIDS+=($!)
    CLONE_NEEDED=$((CLONE_NEEDED + 1))
  fi
done

if [ "$CLONE_NEEDED" -gt 0 ]; then
  echo "  Waiting for $CLONE_NEEDED clone(s)..."
  CLONE_FAIL=0
  for pid in "${PIDS[@]}"; do
    if ! wait "$pid"; then
      CLONE_FAIL=$((CLONE_FAIL + 1))
    fi
  done
  if [ "$CLONE_FAIL" -gt 0 ]; then
    echo "  WARNING: $CLONE_FAIL clone(s) failed"
  fi
  echo "  Done cloning."
else
  echo "  All repos present."
fi
echo ""

# =============================================================================
# Run analysis
# =============================================================================
mkdir -p "$OUTPUT_DIR"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="$OUTPUT_DIR/bench_${TIMESTAMP}.log"
JSON_DIR="$OUTPUT_DIR/json"
mkdir -p "$JSON_DIR"

echo "=== Running Ising Analysis Benchmark ==="
echo "  Date: $(date -Iseconds)"
echo "  Binary: $ISING"
echo "  Repos: $REPOS_DIR"
echo "  Output: $OUTPUT_DIR"
echo "  Since: $SINCE"
echo ""

PASS=0
FAIL=0
SKIP=0

for def in "${REPO_DEFS[@]}"; do
  IFS='|' read -r name url lang category notes <<< "$def"
  REPO_PATH="$REPOS_DIR/$name"
  DB_PATH="$OUTPUT_DIR/${name}.db"

  if [ ! -d "$REPO_PATH" ]; then
    echo "  SKIP: $name (not cloned)"
    echo '{"status":"skip"}' > "$JSON_DIR/${name}_health.json"
    SKIP=$((SKIP + 1))
    continue
  fi

  echo -n "  Analyzing $name... "

  # Build
  if $ISING build --repo-path "$REPO_PATH" --db "$DB_PATH" --since "$SINCE" >>"$LOG_FILE" 2>&1; then
    # Export health as JSON
    if $ISING health --db "$DB_PATH" --format json > "$JSON_DIR/${name}_health.json" 2>/dev/null; then
      GRADE=$(python3 -c "import json; print(json.load(open('$JSON_DIR/${name}_health.json'))['grade'])" 2>/dev/null || echo "?")
      SCORE=$(python3 -c "import json; print(f\"{json.load(open('$JSON_DIR/${name}_health.json'))['score']:.2f}\")" 2>/dev/null || echo "?")
      echo "OK  Grade=$GRADE Score=$SCORE"
      PASS=$((PASS + 1))
    else
      echo "OK  (build succeeded, health failed)"
      FAIL=$((FAIL + 1))
    fi

    # Export stats
    $ISING stats --db "$DB_PATH" --format json > "$JSON_DIR/${name}_stats.json" 2>/dev/null || true
  else
    echo "FAIL (build error)"
    FAIL=$((FAIL + 1))
  fi
done

echo ""
echo "=== Summary: $PASS passed, $FAIL failed, $SKIP skipped ==="
echo ""

# =============================================================================
# Generate results table
# =============================================================================
echo "=== Results Table ==="
echo ""

python3 - "$JSON_DIR" "${REPO_DEFS[@]}" << 'PYEOF'
import json, sys, os

json_dir = sys.argv[1]
repo_defs = sys.argv[2:]

results = []
for defn in repo_defs:
    parts = defn.split("|")
    name, url, lang, category, notes = parts[0], parts[1], parts[2], parts[3], parts[4]

    health_file = os.path.join(json_dir, f"{name}_health.json")
    stats_file = os.path.join(json_dir, f"{name}_stats.json")

    if not os.path.exists(health_file):
        results.append({"name": name, "lang": lang, "category": category, "grade": "FAIL"})
        continue

    try:
        with open(health_file) as f:
            h = json.load(f)

        if h.get("status") == "skip":
            results.append({"name": name, "lang": lang, "category": category, "grade": "SKIP"})
            continue
        sig_count = 0
        if os.path.exists(stats_file):
            with open(stats_file) as f:
                s = json.load(f)
                sig_count = s.get("signal_count", 0)

        results.append({
            "name": name, "lang": lang, "category": category,
            "grade": h["grade"], "score": h["score"],
            "total": h["total_modules"], "active": h["active_modules"],
            "risk": h["risk_sub_score"], "signals": h["signal_sub_score"],
            "structure": h["structural_sub_score"],
            "sig_count": sig_count,
            "critical": h["critical_count"], "high": h.get("high_count", 0),
            "god_density": h["god_module_density"],
            "caveats": h.get("caveats", [])
        })
    except Exception as e:
        results.append({"name": name, "lang": lang, "category": category, "grade": "ERR", "error": str(e)})

# Print table
hdr = f"{'Repository':<25} {'Lang':<8} {'Cat':<8} {'Grade':>5} {'Score':>6} {'Total':>7} {'Active':>7} {'Risk':>6} {'Sigs':>6} {'Struc':>6} {'#Sigs':>7} {'Crit':>5} {'High':>5}"
print(hdr)
print("-" * len(hdr))

for r in results:
    if r["grade"] in ("FAIL", "ERR", "SKIP"):
        print(f"{r['name']:<25} {r['lang']:<8} {r['category']:<8} {r['grade']:>5}")
    else:
        print(f"{r['name']:<25} {r['lang']:<8} {r['category']:<8} {r['grade']:>5} {r['score']:>6.2f} {r['total']:>7} {r['active']:>7} {r['risk']:>6.2f} {r['signals']:>6.2f} {r['structure']:>6.2f} {r['sig_count']:>7} {r['critical']:>5} {r['high']:>5}")

# Calibration checks
print()
print("=== Calibration Checks ===")
gin = next((r for r in results if r["name"] == "gin"), None)
if gin and gin["grade"] not in ("FAIL", "SKIP"):
    status = "PASS" if gin["grade"] in ("A", "B") else "FAIL"
    print(f"  [{status}] gin >= B: got {gin['grade']} ({gin['score']:.2f})")

odoo = next((r for r in results if r["name"] == "odoo"), None)
if odoo and odoo["grade"] not in ("FAIL", "SKIP"):
    status = "WARN" if odoo["grade"] == "A" else "PASS"
    print(f"  [{status}] odoo != A (known blind spot): got {odoo['grade']} ({odoo['score']:.2f})")

ts = next((r for r in results if r["name"] == "TypeScript"), None)
if ts and ts["grade"] not in ("FAIL", "SKIP"):
    status = "WARN" if ts["grade"] == "A" and ts["score"] > 0.95 else "OK"
    print(f"  [{status}] TypeScript sanity: got {ts['grade']} ({ts['score']:.2f}) - {ts['total']} modules")

# Grade distribution
print()
print("=== Grade Distribution ===")
from collections import Counter
grades = Counter(r["grade"] for r in results)
for g in ["A", "B", "C", "D", "F", "FAIL", "SKIP", "ERR"]:
    if g in grades:
        names = [r["name"] for r in results if r["grade"] == g]
        print(f"  {g}: {grades[g]}  ({', '.join(names)})")
PYEOF

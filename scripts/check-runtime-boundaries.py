"""Fail if a runtime shell acquires private archive tooling dependencies."""
import subprocess

for crate in ["raydium-investigation", "raydium-debugger-server", "raydium-debugger-tauri"]:
    dependencies = subprocess.check_output(["cargo", "tree", "-p", crate, "--edges", "normal", "--prefix", "none"], text=True)
    for forbidden in ["raydium-knowledge-builder ", "scraper ", "petgraph ", "tantivy "]:
        assert forbidden not in dependencies, f"{crate} depends on private tooling: {forbidden}"
print("Runtime dependency boundaries passed for investigation, HTTP and desktop.")

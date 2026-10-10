#!/usr/bin/env python3
import os, sys, json, subprocess, time

GAUNTLET_DIR = "/Users/stompinggrounds/Development/filegoblin/test_assets/gauntlet"
MANIFEST_PATH = f"{GAUNTLET_DIR}/gauntlet_manifest.json"
BINARY_PATH = "/Users/stompinggrounds/Development/filegoblin/target/debug/filegoblin"

if not os.path.exists(BINARY_PATH):
    BINARY_PATH = "filegoblin"

with open(MANIFEST_PATH) as f:
    manifest = json.load(f)

print("=" * 70)
print(f"       FILEGOBLIN ADVERSARIAL GAUNTLET (v1.14.0)")
print("=" * 70)

results = []
passed_count = 0

for item in manifest:
    case_id = item["id"]
    filename = item["file"]
    filepath = os.path.join(GAUNTLET_DIR, filename)
    desc = item["description"]
    
    start = time.time()
    res = subprocess.run([BINARY_PATH, filepath, "-q"], capture_output=True, text=True)
    elapsed = time.time() - start
    
    stdout = res.stdout
    stderr = res.stderr
    exit_code = res.returncode
    
    errors = []
    if exit_code != 0:
        errors.append(f"Non-zero exit ({exit_code}): {stderr.strip()[:100]}")
    
    # Check canaries present
    for canary in item.get("canaries_present", []):
        if canary not in stdout:
            errors.append(f"Missing canary: '{canary}'")
            
    # Check forbidden absent
    for forbidden in item.get("forbidden_absent", []):
        if forbidden in stdout:
            errors.append(f"Found forbidden token: '{forbidden}'")
            
    # Check syntax contains
    for syn in item.get("syntax_contains", []):
        if syn not in stdout:
            errors.append(f"Missing syntax marker: '{syn}'")
            
    passed = len(errors) == 0
    if passed:
        passed_count += 1
        status = "\033[92mPASS\033[0m"
    else:
        status = "\033[91mFAIL\033[0m"
        
    print(f"[{status}] {case_id:<3}: {desc[:48]:<48} ({elapsed*1000:.1f}ms)")
    if errors:
        for err in errors[:3]:
            print(f"       -> {err}")
            
    results.append({
        "id": case_id,
        "file": filename,
        "category": item["category"],
        "description": desc,
        "passed": passed,
        "elapsed_ms": elapsed * 1000,
        "errors": errors
    })

score = (passed_count / len(manifest)) * 100
print("=" * 70)
print(f"GAUNTLET SCORE: {passed_count}/{len(manifest)} ({score:.1f}%)")
print("=" * 70)

# Generate Markdown scorecard
scorecard_path = "/Users/stompinggrounds/Development/filegoblin/GAUNTLET_SCORECARD.md"
with open(scorecard_path, "w") as f:
    f.write("# Filegoblin Adversarial Gauntlet Scorecard\n\n")
    f.write(f"**Overall Score:** {passed_count}/{len(manifest)} ({score:.1f}%)\n\n")
    f.write("| ID | Category | Description | Status | Latency | Primary Failure Reason |\n")
    f.write("| :--- | :--- | :--- | :--- | :--- | :--- |\n")
    for r in results:
        status_md = "✅ PASS" if r["passed"] else "❌ FAIL"
        err_msg = r["errors"][0] if r["errors"] else "None"
        f.write(f"| **{r['id']}** | {r['category']} | {r['description']} | {status_md} | {r['elapsed_ms']:.1f} ms | {err_msg} |\n")

print(f"Scorecard written to: {scorecard_path}")
sys.exit(0 if passed_count == len(manifest) else 1)

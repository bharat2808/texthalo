#!/usr/bin/env python3
"""Regenerate the ported Misaki phone mappings from pinned, verified upstream files.
Developer tool only; TextHalo's native front ends do not require Python.
"""
import ast
import hashlib
import json
from pathlib import Path
from urllib.request import urlopen

COMMIT = 'fba1236595f2d2bf21d414ba6e57d25256afada3'
DIGESTS = {'cutlet.py': '7695fc7d853ae4e99679dff4ca0c8404d4804a6fb1059a9d6ad02baeabc4d4de', 'transcription.py': '14f0f1b5eb89c5b684229a30ae8b0c19c8266b537967786a2079ba887f5fbc32'}
ROOT = Path(__file__).resolve().parents[1] / "src-tauri/src/cjk_data"

sources = {}
for name, digest in DIGESTS.items():
    source = urlopen(f"https://raw.githubusercontent.com/hexgrad/misaki/{COMMIT}/misaki/{name}").read()
    if hashlib.sha256(source).hexdigest() != digest:
        raise RuntimeError(f"upstream source digest mismatch: {name}")
    sources[name] = source.decode()

# Execute only the phone mapping literals, never upstream imports or runtime code.
environment = {"chr": chr}
for node in ast.parse(sources["cutlet.py"]).body:
    assignment = isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "HEPBURN" for t in node.targets)
    update = isinstance(node, ast.Expr) and isinstance(node.value, ast.Call) and isinstance(node.value.func, ast.Attribute) and isinstance(node.value.func.value, ast.Name) and node.value.func.value.id == "HEPBURN" and node.value.func.attr == "update"
    if assignment or update:
        exec(compile(ast.Module(body=[node], type_ignores=[]), "verified-phone-mappings", "exec"), environment)

mappings = {}
for node in ast.parse(sources["transcription.py"]).body:
    if isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name) and "MAPPING" in node.target.id:
        mappings[node.target.id] = {k: "".join(v[0]) for k, v in ast.literal_eval(node.value).items()}
for name, data in [("ja_kana.json", environment["HEPBURN"]), ("zh_ipa.json", mappings)]:
    (ROOT / name).write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")

"""Export the pinned MIT FCPE checkpoint and verify the ONNX graph against PyTorch.

Run from the repository root, outside the audio runtime:
    uv venv --python 3.12 target/revox/venv
    uv pip install --python target/revox/venv/bin/python --require-hashes -r scripts/revox/requirements.lock
    target/revox/venv/bin/python scripts/revox/export_fcpe.py

Python is used here because the upstream checkpoint and ONNX exporter use PyTorch.
The shipped Rust processor does not load Python. Downloaded sources, weights,
ONNX and generated evidence remain under target/revox.

Exports fcpe.onnx for the variable-length mono DSP probe and fcpe-plugin.onnx
for the plug-in's 101760-sample 48 kHz window. The latter fixes time at 213 FCPE
frames and keeps only axis 0 symbolic ("batch"). Each worker specializes that
symbol to its mono/stereo channel count through ORT's dimension override;
there is one embedded set of FCPE weights for both bus layouts.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import importlib.metadata
import json
from pathlib import Path
import sys
import types
import urllib.request

import numpy as np
import onnx
import onnxruntime as ort
import torch


SCRIPT_DIR = Path(__file__).resolve().parent
ROOT = SCRIPT_DIR.parent.parent
PLUGIN_FRAMES = 213


def sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def acquire_source(output: Path, source: dict) -> Path:
    upstream = output / "fcpe-upstream"
    base = f"https://raw.githubusercontent.com/CNChTu/FCPE/{source['revision']}/"
    for relative, expected in source["files"].items():
        destination = upstream / relative
        if destination.exists() and sha256(destination) == expected:
            continue
        destination.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(base + relative, timeout=120) as response:
            data = response.read()
        actual = hashlib.sha256(data).hexdigest()
        if actual != expected:
            raise ValueError(f"FCPE source checksum mismatch: {relative}: {actual}")
        destination.write_bytes(data)
    return upstream


def load_reference(upstream: Path, source: dict):
    # Load the original model modules without torchfcpe.__init__, which imports
    # the unrelated MIDI application and its additional dependencies.
    package = types.ModuleType("torchfcpe")
    package.__path__ = [str(upstream / "torchfcpe")]
    sys.modules["torchfcpe"] = package
    models = importlib.import_module("torchfcpe.models")
    features = importlib.import_module("torchfcpe.mel_extractor")
    checkpoint = torch.load(
        upstream / source["checkpoint"], map_location="cpu", weights_only=True
    )
    config = checkpoint["config_dict"]
    model_config = config["model"]
    model = models.CFNaiveMelPE(
        input_channels=config["mel"]["num_mels"],
        out_dims=model_config["out_dims"],
        hidden_dims=model_config["hidden_dims"],
        n_layers=model_config["n_layers"],
        n_heads=model_config["n_heads"],
        f0_min=model_config["f0_min"],
        f0_max=model_config["f0_max"],
        use_fa_norm=model_config["use_fa_norm"],
        conv_only=model_config["conv_only"],
        conv_dropout=0.0,
        atten_dropout=0.0,
    )
    model.load_state_dict(checkpoint["model"], strict=True)
    model.eval()
    mel = features.Wav2MelModule(16000, 128, 1024, 1024, 160, 0, 8000)
    return model, mel, config


def check_parity(model, graph: Path) -> list[dict]:
    session = ort.InferenceSession(str(graph), providers=["CPUExecutionProvider"])
    rng = np.random.default_rng(417)
    evidence = []
    for frames in (1, 17, 65, 101):
        mel = rng.uniform(-11.5, 2.0, size=(1, frames, 128)).astype(np.float32)
        with torch.inference_mode():
            reference = model(torch.from_numpy(mel)).numpy()
        actual = session.run(["probabilities"], {"mel": mel})[0]
        np.testing.assert_allclose(actual, reference, atol=2e-5, rtol=2e-4)
        evidence.append({
            "frames": frames,
            "maximum_absolute_error": float(np.max(np.abs(actual - reference))),
        })
    return evidence


def make_mel_fixtures(mel, source: dict) -> dict:
    cases = []
    for count in (2049, 1):
        time = np.arange(count, dtype=np.float64) / 16000.0
        audio = (0.3 * np.sin(2 * np.pi * 220 * time)
                 + 0.1 * np.cos(2 * np.pi * 660 * time)).astype(np.float32)
        if count == 1:
            audio[0] = 0.25
        with torch.inference_mode():
            actual = mel(torch.from_numpy(audio)[None, :, None], 16000).numpy()[0]
        frames = (0, 1, 6, 12) if count == 2049 else (0,)
        bands = (0, 12, 31, 64, 100, 127) if count == 2049 else (0, 12, 64, 127)
        cases.append({
            "sample_count": count,
            "input_description": (
                "f32(0.3*sin(2*pi*220*i/16000)+0.1*cos(2*pi*660*i/16000)), calculated in f64"
                if count == 2049 else "single sample 0.25"
            ),
            "expected_frames": int(actual.shape[0]),
            "samples": [
                {"frame": frame, "mel": band, "value": float(actual[frame, band])}
                for frame in frames for band in bands
            ],
        })
    return {"source_commit": source["revision"], "cases": cases}


def check_plugin_parity(model, frontend, graph: Path) -> list[dict]:
    # Two distinct channels exercise the layout our exported graph owns. Compare
    # against independent mono PyTorch calls, so batch mixing cannot agree with
    # an equally batched reference and pass unnoticed.
    time = np.arange(33_920, dtype=np.float64) / 16_000.0
    audio = np.stack([
        0.3 * np.sin(2 * np.pi * 220 * time) + 0.08 * np.cos(2 * np.pi * 660 * time),
        0.2 * np.sin(2 * np.pi * 330 * time) + 0.06 * np.cos(2 * np.pi * 990 * time),
    ]).astype(np.float32)
    with torch.inference_mode():
        harmonic_mel = torch.cat([
            frontend(torch.from_numpy(channel)[None, :, None], 16_000)
            for channel in audio
        ]).numpy()
    assert harmonic_mel.shape == (2, PLUGIN_FRAMES, 128)
    rng = np.random.default_rng(417)
    cases = [
        ("distinct_harmonics", harmonic_mel),
        ("random_mel", rng.uniform(-11.5, 2.0, (2, PLUGIN_FRAMES, 128)).astype(np.float32)),
    ]

    def session(batch: int | None):
        options = ort.SessionOptions()
        options.intra_op_num_threads = 1
        if batch is not None:
            options.add_free_dimension_override_by_name("batch", batch)
        # Untimed export correctness only; runtime/provider performance belongs
        # to Rust's default-provider DSP and plug-in validation.
        return ort.InferenceSession(
            str(graph), sess_options=options, providers=["CPUExecutionProvider"]
        )

    dynamic = session(None)
    specialized = {batch: session(batch) for batch in (1, 2)}
    for batch, concrete in specialized.items():
        assert concrete.get_inputs()[0].shape == [batch, PLUGIN_FRAMES, 128]
        assert concrete.get_outputs()[0].shape == [batch, PLUGIN_FRAMES, 360]
    evidence = []
    for name, features in cases:
        with torch.inference_mode():
            independent = torch.cat([
                model(torch.from_numpy(channel[None])) for channel in features
            ]).numpy()
            batched = model(torch.from_numpy(features)).numpy()
        np.testing.assert_allclose(batched, independent, atol=2e-5, rtol=2e-4)
        for batch in (1, 2):
            reference = independent[:batch]
            dynamic_output = dynamic.run(["probabilities"], {"mel": features[:batch]})[0]
            actual = specialized[batch].run(["probabilities"], {"mel": features[:batch]})[0]
            assert actual.shape == dynamic_output.shape == (batch, PLUGIN_FRAMES, 360)
            np.testing.assert_allclose(dynamic_output, reference, atol=2e-5, rtol=2e-4)
            np.testing.assert_allclose(actual, reference, atol=2e-5, rtol=2e-4)
            evidence.append({
                "case": name,
                "batch": batch,
                "maximum_absolute_error_vs_independent_pytorch": float(
                    np.max(np.abs(actual - reference))
                ),
                "unspecialized_maximum_absolute_error_vs_independent_pytorch": float(
                    np.max(np.abs(dynamic_output - reference))
                ),
                "source_batch_independence_maximum_absolute_error": float(
                    np.max(np.abs(batched[:batch] - reference))
                ),
            })
    return evidence


def export_plugin(model, frontend, output: Path, source: dict, config: dict,
                  packages: dict) -> dict:
    graph = output / "fcpe-plugin.onnx"
    dynamic_axes = {"mel": {0: "batch"}, "probabilities": {0: "batch"}}
    with torch.inference_mode():
        torch.onnx.export(
            model, torch.zeros(1, PLUGIN_FRAMES, 128, dtype=torch.float32), graph,
            input_names=["mel"], output_names=["probabilities"],
            dynamic_axes=dynamic_axes, opset_version=17, dynamo=False,
        )
    onnx.checker.check_model(str(graph), full_check=True)
    parsed = onnx.load(str(graph))

    def shape(value):
        return [dimension.dim_param or dimension.dim_value
                for dimension in value.type.tensor_type.shape.dim]

    assert len(parsed.graph.input) == len(parsed.graph.output) == 1
    assert shape(parsed.graph.input[0]) == ["batch", PLUGIN_FRAMES, 128]
    assert shape(parsed.graph.output[0]) == ["batch", PLUGIN_FRAMES, 360]
    parity = check_plugin_parity(model, frontend, graph)
    evidence = {
        "upstream": source,
        "onnx": {"file": graph.name, "sha256": sha256(graph), "bytes": graph.stat().st_size},
        "input": {"name": "mel", "dtype": "float32", "shape": ["batch", PLUGIN_FRAMES, 128]},
        "output": {"name": "probabilities", "dtype": "float32", "shape": ["batch", PLUGIN_FRAMES, 360]},
        "runtime_dimension_override": {"name": "batch", "supported_values": [1, 2]},
        "window": {"samples_48000_hz": 101_760, "samples_16000_hz": 33_920},
        "mel_config": config["mel"],
        "model_config": config["model"],
        "export": {"opset": 17, "dynamo": False, "dynamic_axes": dynamic_axes},
        "onnx_checker": "passed (full_check)",
        "parity_tolerance": {"absolute": 2e-5, "relative": 2e-4},
        "parity": parity,
        "packages": packages,
        "requirements_lock_sha256": sha256(SCRIPT_DIR / "requirements.lock"),
        "export_script_sha256": sha256(Path(__file__)),
        "performance_measurements": None,
    }
    (output / "fcpe-plugin-export.json").write_text(json.dumps(evidence, indent=2) + "\n")
    return evidence


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/revox")
    args = parser.parse_args()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = json.loads((SCRIPT_DIR / "fcpe-source.json").read_text())
    upstream = acquire_source(output, source)
    torch.set_num_threads(1)
    model, mel, config = load_reference(upstream, source)
    graph = output / "fcpe.onnx"
    example = torch.zeros(1, 65, 128, dtype=torch.float32)
    with torch.inference_mode():
        torch.onnx.export(
            model, example, graph, input_names=["mel"],
            output_names=["probabilities"],
            dynamic_axes={"mel": {1: "frames"}, "probabilities": {1: "frames"}},
            opset_version=17, dynamo=False,
        )
    onnx.checker.check_model(str(graph))
    parity = check_parity(model, graph)
    fixtures = make_mel_fixtures(mel, source)
    (output / "fcpe-mel-reference.json").write_text(json.dumps(fixtures, indent=2) + "\n")
    evidence = {
        "upstream": source,
        "onnx": {"file": graph.name, "sha256": sha256(graph), "bytes": graph.stat().st_size},
        "input": {"name": "mel", "dtype": "float32", "shape": [1, "frames", 128]},
        "output": {"name": "probabilities", "dtype": "float32", "shape": [1, "frames", 360]},
        "mel_config": config["mel"],
        "model_config": config["model"],
        "opset": 17,
        "parity": parity,
        "packages": {name: importlib.metadata.version(name) for name in (
            "torch", "torchaudio", "numpy", "onnx", "onnxruntime", "einops",
            "local-attention", "librosa",
        )},
    }
    (output / "fcpe-export.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps({"onnx": evidence["onnx"], "parity": parity}, indent=2))
    plugin = export_plugin(model, mel, output, source, config, evidence["packages"])
    print(json.dumps({"onnx": plugin["onnx"], "parity": plugin["parity"]}, indent=2))


if __name__ == "__main__":
    main()

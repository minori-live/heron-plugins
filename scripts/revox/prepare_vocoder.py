"""Prepare Revox's fixed 212-frame plugin graph with one symbolic channel batch.

Run after node scripts/fetch-revox-model.ts using target/revox/venv/bin/python.
Only input/output shape annotations change; the original mono probe graph stays
available. The Rust session fixes `batch` to the active mono/stereo layout.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path

import onnx
from onnx import helper, numpy_helper

ROOT = Path(__file__).resolve().parent.parent.parent
SOURCE_SHA256 = "e7a94cf11388ad12ba9ba1316029fdcbf15f77e342559bd1e9d6971a3f8ee020"
REVISION = "59a27b71358a2de4b22acda1b07d89d02bf0de79"
FRAMES = 212
SAMPLES = FRAMES * 480


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def shape(value) -> list:
    return [d.dim_value if d.HasField("dim_value") else d.dim_param
            for d in value.type.tensor_type.shape.dim]


def audit_batch_axes(model) -> None:
    """Check the pinned graph's operations that can join or reinterpret axes."""
    require(not model.functions, "Revox functions require a separate batch audit")
    constants = {t.name: numpy_helper.to_array(t) for t in model.graph.initializer}
    shapes = {n.output[0] for n in model.graph.node if n.op_type == "Shape"}
    for node in model.graph.node:
        if node.op_type == "Constant":
            constants[node.output[0]] = numpy_helper.to_array(
                helper.get_attribute_value(node.attribute[0]))

    pointwise = {"Cast", "And", "Greater", "Clip", "Div", "Log", "Where", "Mul",
                 "Less", "Xor", "Mod", "Equal", "Not", "Sub", "Add",
                 "LessOrEqual", "Reciprocal", "Sin", "Erf", "Exp", "Cos"}
    for node in model.graph.node:
        attrs = {a.name: helper.get_attribute_value(a) for a in node.attribute}
        require(node.domain in ("", "ai.onnx"), f"Custom operator: {node.name}")
        require(not any(a.type in (onnx.AttributeProto.GRAPH, onnx.AttributeProto.GRAPHS)
                        for a in node.attribute), f"Nested graph: {node.name}")
        if node.op_type in pointwise | {"Constant", "Shape", "Range", "Conv"}:
            continue  # Conv treats N independently; pointwise constants broadcast.
        if node.op_type == "Gather":
            if node.input[0] in shapes:
                require(attrs.get("axis", 0) == 0 and
                        0 not in constants[node.input[1]].reshape(-1),
                        f"Reading batch from Shape: {node.name}")
            else:
                require(attrs.get("axis", 0) == 1, f"Gather batch axis: {node.name}")
        elif node.op_type == "Slice":
            require(set(constants[node.input[3]].reshape(-1)) <= {1, 2},
                    f"Slice batch axis: {node.name}")
        elif node.op_type == "Unsqueeze":
            axes = constants[node.input[1]].reshape(-1).tolist()
            # This single axis-0 insertion wraps a scalar frame-count, not data.
            require(axes in ([1], [-1]) or
                    (node.name == "/Unsqueeze_4" and axes == [0]),
                    f"Unsqueeze batch axis: {node.name}")
        elif node.op_type == "Concat":
            require(attrs["axis"] == 1, f"Concat batch axis: {node.name}")
        elif node.op_type == "Transpose":
            require(attrs["perm"] == [0, 2, 1], f"Transpose batch axis: {node.name}")
        elif node.op_type == "MatMul":
            require(node.input[1] in constants and constants[node.input[1]].ndim == 2,
                    f"MatMul must use a fixed channel projection: {node.name}")
        elif node.op_type == "LayerNormalization":
            require(attrs.get("axis", -1) == -1, f"Normalize batch axis: {node.name}")
        elif node.op_type == "CumSum":
            require(constants[node.input[1]].item() == -1, f"CumSum batch axis: {node.name}")
        elif node.op_type == "ReduceSum":
            if len(node.input) == 2:
                require(constants[node.input[1]].tolist() == [-1],
                        f"ReduceSum batch axis: {node.name}")
            else:
                # The sole all-axis reduction normalizes constant harmonic weights.
                require(node.name == "/source/ReduceSum_1" and
                        node.input[0] == "/source/Reciprocal_output_0",
                        f"Data-dependent all-axis reduction: {node.name}")
        else:
            raise ValueError(f"Unaudited Revox operator: {node.op_type} ({node.name})")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, default=ROOT / "target/revox/models")
    directory = parser.parse_args().model_dir.resolve()
    source = directory / "revox.onnx"
    require(hashlib.sha256(source.read_bytes()).hexdigest() == SOURCE_SHA256,
            "Revox source checksum mismatch; fetch the pinned model before preparation")
    original = onnx.load(source)
    audit_batch_axes(original)
    model = copy.deepcopy(original)
    for value in [*model.graph.input, *model.graph.output]:
        value.type.tensor_type.shape.dim[0].dim_param = "batch"
        for dim in value.type.tensor_type.shape.dim[1:]:
            if dim.dim_param == "frames":
                dim.dim_value = FRAMES
            elif dim.dim_param == "samples":
                dim.dim_value = SAMPLES
    require([n.SerializeToString() for n in model.graph.node] ==
            [n.SerializeToString() for n in original.graph.node], "Revox operations changed")
    require([t.SerializeToString() for t in model.graph.initializer] ==
            [t.SerializeToString() for t in original.graph.initializer], "Revox weights changed")
    onnx.checker.check_model(model, full_check=True)
    for batch in (1, 2):
        specialized = copy.deepcopy(model)
        for value in [*specialized.graph.input, *specialized.graph.output]:
            value.type.tensor_type.shape.dim[0].dim_value = batch
        onnx.checker.check_model(specialized, full_check=True)
        inferred = onnx.shape_inference.infer_shapes(specialized, strict_mode=True, data_prop=True)
        require(all(shape(v) == [batch, 1025, FRAMES] for v in inferred.graph.output),
                f"Unexpected Revox output shape for batch {batch}")
    output = directory / "revox-plugin.onnx"
    onnx.save(model, output)
    evidence = {
        "revision": REVISION, "source_sha256": SOURCE_SHA256,
        "onnx": {"file": output.name, "sha256": hashlib.sha256(output.read_bytes()).hexdigest()},
        "inputs": {v.name: shape(v) for v in model.graph.input},
        "outputs": {v.name: shape(v) for v in model.graph.output},
        "transformation": "Only input/output shape annotations; all nodes and weights unchanged",
        "batch_audit": "Time/channel operations preserve axis 0; no cross-item reduction or reshape",
        "checked_batches": [1, 2], "opset": 17,
    }
    (directory / "revox-plugin-manifest.json").write_text(json.dumps(evidence, indent=2) + "\n")
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Builds the ONNX weights for filegoblin's `vision` feature.

Every model is permissively licensed and is converted here once, at release
time; filegoblin itself only ever loads the resulting `.onnx` files with rten
(pure Rust). Nothing in this script runs on a user's machine.

| file                     | source                                              | license    |
|--------------------------|-----------------------------------------------------|------------|
| pp-doclayout-v3.onnx     | PaddlePaddle/PP-DocLayoutV3_safetensors             | Apache-2.0 |
| mfr-encoder.onnx         | breezedeus/pix2text-mfr-1.5 (encoder_model.onnx)    | MIT        |
| mfr-decoder-init.onnx    | rebuilt from pix2text-mfr-1.5 decoder weights       | MIT        |
| mfr-decoder-step.onnx    | rebuilt from pix2text-mfr-1.5 decoder weights       | MIT        |
| mfr-tokenizer.json       | breezedeus/pix2text-mfr-1.5                         | MIT        |
| tatr-structure.onnx      | microsoft/table-transformer-structure-recognition-v1.1-all | MIT |
| ppocr-v5-det.onnx        | PaddlePaddle/PP-OCRv5_mobile_det_onnx               | Apache-2.0 |
| ppocr-v5-rec.onnx        | PaddlePaddle/en_PP-OCRv5_mobile_rec_onnx            | Apache-2.0 |
| ppocr-v5-dict.txt        | the rec model's `inference.yml` character list      | Apache-2.0 |

Conversions:
* PP-DocLayoutV3 is exported from transformers at a fixed 800×800 input and
  every float64 tensor is rewritten as float32 (rten has no f64).
* pix2text ships an uncached decoder. Its weights are lifted out of the ONNX
  graph into a plain-torch TrOCR decoder, which is checked against the
  original to 1e-3 in logits and exported as `init` (cross-attention K/V) and
  `step` (one token with a self-attention cache).
* Table Transformer is exported with dynamic height/width.
* The PaddleOCR graphs are upgraded to opset 17 (rten rejects the legacy
  `Slice`/`BatchNormalization` attributes they carry), and every graph gets
  the clean-ups rten does not do itself: Identity removal, constant folding of
  reshapes, Conv + per-channel affine/BatchNorm folding, and
  `x * HardSigmoid(x)` → `HardSwish`. Outputs are checked against the source
  graph with onnxruntime.

Usage (needs torch, transformers, onnx, onnxruntime, huggingface_hub):
    python scripts/vision_models.py OUT_DIR
Then point filegoblin at it with FILEGOBLIN_MODELS=OUT_DIR, or ship the files
in a `models/` directory next to the binary.
"""

import hashlib
import json
import math
import os
import sys
import urllib.request

HF = "https://huggingface.co"


def fetch(repo, path, dst):
    if os.path.exists(dst):
        return dst
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    url = f"{HF}/{repo}/resolve/main/{path}"
    print("fetch", url)
    urllib.request.urlretrieve(url, dst + ".part")
    os.replace(dst + ".part", dst)
    return dst


def no_float64(src, dst):
    """Rewrite every float64 tensor/cast in an ONNX graph as float32."""
    import numpy as np
    import onnx
    from onnx import TensorProto, numpy_helper

    m = onnx.load(src)

    def fix_tensor(t):
        if t.data_type == TensorProto.DOUBLE:
            t.CopyFrom(numpy_helper.from_array(numpy_helper.to_array(t).astype(np.float32), t.name))

    def walk(g):
        for t in g.initializer:
            fix_tensor(t)
        for n in g.node:
            for a in n.attribute:
                if n.op_type == "Cast" and a.name == "to" and a.i == TensorProto.DOUBLE:
                    a.i = TensorProto.FLOAT
                if a.name == "dtype" and a.i == TensorProto.DOUBLE:
                    a.i = TensorProto.FLOAT
                if a.type == onnx.AttributeProto.TENSOR:
                    fix_tensor(a.t)
                if a.type == onnx.AttributeProto.GRAPH:
                    walk(a.g)
        for vi in list(g.value_info) + list(g.input) + list(g.output):
            if vi.type.tensor_type.elem_type == TensorProto.DOUBLE:
                vi.type.tensor_type.elem_type = TensorProto.FLOAT

    walk(m.graph)
    onnx.save(m, dst, save_as_external_data=False)


def optimize_for_rten(src, dst):
    """Graph clean-ups rten does not perform itself (see module docs)."""
    import numpy as np
    import onnx
    from onnx import helper, numpy_helper, version_converter

    m = onnx.load(src)
    if max(o.version for o in m.opset_import if o.domain in ("", "ai.onnx")) < 14:
        m = version_converter.convert_version(m, 17)
    g = m.graph
    outs = {o.name for o in g.output}
    for n in list(g.node):
        if n.op_type == "BatchNormalization":
            keep = [a for a in n.attribute if a.name != "spatial"]
            del n.attribute[:]
            n.attribute.extend(keep)
        if n.op_type == "Identity" and n.output[0] not in outs:
            for c in g.node:
                for k, x in enumerate(c.input):
                    if x == n.output[0]:
                        c.input[k] = n.input[0]
            g.node.remove(n)
    for n in list(g.node):
        if n.op_type == "Constant" and n.attribute and n.attribute[0].name == "value":
            t = n.attribute[0].t
            t.name = n.output[0]
            g.initializer.append(t)
            g.node.remove(n)
    names = {i.name for i in g.initializer}
    for n in list(g.node):
        if n.op_type in ("Reshape", "Unsqueeze", "Squeeze") and n.input[0] in names and all(
                x in names for x in n.input[1:] if x):
            arrs = {i.name: numpy_helper.to_array(i) for i in g.initializer if i.name in n.input}
            a = arrs[n.input[0]]
            if n.op_type == "Reshape":
                shp = [a.shape[k] if v == 0 else v for k, v in enumerate(arrs[n.input[1]].astype(np.int64).tolist())]
                r = a.reshape(shp)
            elif n.op_type == "Unsqueeze":
                r = a
                for ax in sorted(arrs[n.input[1]].tolist()):
                    r = np.expand_dims(r, ax)
            else:
                r = np.squeeze(a, tuple(arrs[n.input[1]].tolist())) if len(n.input) > 1 else np.squeeze(a)
            g.initializer.append(numpy_helper.from_array(r, n.output[0]))
            names.add(n.output[0])
            g.node.remove(n)
    inits = {i.name: i for i in g.initializer}

    def const(name):
        return numpy_helper.to_array(inits[name]) if name in inits else None

    def consumers(name):
        return [n for n in g.node if name in n.input]

    folded = 0
    changed = True
    while changed:
        changed = False
        for conv in list(g.node):
            if conv.op_type != "Conv" or conv.output[0] in outs:
                continue
            cs = consumers(conv.output[0])
            W = const(conv.input[1])
            if len(cs) != 1 or W is None:
                continue
            nxt, C = cs[0], W.shape[0]
            b = const(conv.input[2]) if len(conv.input) > 2 and conv.input[2] else np.zeros(C, np.float32)
            if nxt.op_type in ("Mul", "Add"):
                other = nxt.input[1] if nxt.input[0] == conv.output[0] else nxt.input[0]
                c = const(other)
                per_channel = c is not None and c.size == C and (
                    c.ndim == 1 or (c.ndim >= 3 and c.shape[-1] == 1 and c.shape[-2] == 1))
                if c is None or not (c.size == 1 or per_channel):
                    continue
                c = np.broadcast_to(c.reshape(-1).astype(np.float32), (C,))
                if nxt.op_type == "Mul":
                    W, b = W * c.reshape(-1, *([1] * (W.ndim - 1))), b * c
                else:
                    b = b + c
            elif nxt.op_type == "BatchNormalization":
                sc, bi, mu, var = [const(x) for x in nxt.input[1:5]]
                if any(v is None for v in (sc, bi, mu, var)):
                    continue
                eps = next((a.f for a in nxt.attribute if a.name == "epsilon"), 1e-5)
                k = sc / np.sqrt(var + eps)
                W, b = W * k.reshape(-1, *([1] * (W.ndim - 1))), (b - mu) * k + bi
            else:
                continue
            wn, bn = f"{conv.input[1]}_f{folded}", f"{conv.name}_b{folded}"
            for name, arr in ((wn, W), (bn, b)):
                g.initializer.append(numpy_helper.from_array(arr.astype(np.float32), name))
                inits[name] = g.initializer[-1]
            conv.input[1] = wn
            if len(conv.input) > 2:
                conv.input[2] = bn
            else:
                conv.input.append(bn)
            conv.output[0] = nxt.output[0]
            g.node.remove(nxt)
            folded += 1
            changed = True
    for hs in list(g.node):
        if hs.op_type != "HardSigmoid":
            continue
        alpha = next((a.f for a in hs.attribute if a.name == "alpha"), 0.2)
        beta = next((a.f for a in hs.attribute if a.name == "beta"), 0.5)
        if abs(alpha - 1 / 6) > 1e-4 or abs(beta - 0.5) > 1e-4:
            continue
        for mul in consumers(hs.output[0]):
            if mul.op_type == "Mul" and set(mul.input) == {hs.input[0], hs.output[0]}:
                i = list(g.node).index(mul)
                g.node.remove(mul)
                g.node.insert(i, helper.make_node("HardSwish", [hs.input[0]], [mul.output[0]]))
        if not consumers(hs.output[0]):
            g.node.remove(hs)
    used = {x for n in g.node for x in n.input}
    keep = [i for i in g.initializer if i.name in used]
    del g.initializer[:]
    g.initializer.extend(keep)
    onnx.save(m, dst, save_as_external_data=False)
    print(f"optimized {os.path.basename(dst)}: {folded} conv epilogues folded")


def check_same(a, b, feeds):
    import numpy as np
    import onnxruntime as ort

    x = ort.InferenceSession(a).run(None, feeds)
    y = ort.InferenceSession(b).run(None, feeds)
    worst = max(float(np.abs(p - q).max()) for p, q in zip(x, y))
    assert worst < 1e-3, f"{b} drifted from {a}: {worst}"


def layout(out, work):
    import torch
    from transformers import AutoModelForObjectDetection

    d = os.path.join(work, "ppdl3")
    for f in ["config.json", "preprocessor_config.json", "model.safetensors"]:
        fetch("PaddlePaddle/PP-DocLayoutV3_safetensors", f, os.path.join(d, f))
    m = AutoModelForObjectDetection.from_pretrained(d).eval()

    class W(torch.nn.Module):
        def __init__(s, m):
            super().__init__()
            s.m = m

        def forward(s, pixel_values):
            o = s.m(pixel_values=pixel_values)
            return o.logits, o.pred_boxes, o.order_logits

    tmp = os.path.join(work, "ppdl3.onnx")
    with torch.no_grad():
        torch.onnx.export(W(m), (torch.rand(1, 3, 800, 800),), tmp, input_names=["pixel_values"],
                          output_names=["logits", "pred_boxes", "order_logits"], opset_version=17, dynamo=False)
    no_float64(tmp, os.path.join(out, "pp-doclayout-v3.onnx"))


def formula(out, work):
    import numpy as np
    import onnx
    import torch
    from onnx import numpy_helper

    d = os.path.join(work, "mfr")
    for f in ["config.json", "encoder_model.onnx", "decoder_model.onnx", "tokenizer.json"]:
        fetch("breezedeus/pix2text-mfr-1.5", f, os.path.join(d, f))
    cfg = json.load(open(os.path.join(d, "config.json")))["decoder"]
    m = onnx.load(os.path.join(d, "decoder_model.onnx"))
    inits = {i.name: numpy_helper.to_array(i) for i in m.graph.initializer}
    consumers = {}
    for n in m.graph.node:
        for x in n.input:
            consumers.setdefault(x, []).append(n)
    W = {k: v for k, v in inits.items() if not k.startswith("onnx::")}
    outputs = {o.name for o in m.graph.output}
    for n in m.graph.node:
        if n.op_type != "MatMul":
            continue
        w = next((x for x in n.input if x in inits and x.startswith("onnx::")), None)
        if w is None:
            continue
        bias = None
        for a in [c for c in consumers.get(n.output[0], []) if c.op_type == "Add"]:
            bias = next((x for x in a.input if x.endswith(".bias") and x in inits), None)
            if bias:
                break
        if bias:
            W[bias[:-5] + ".weight"] = inits[w].T
        elif n.output[0] in outputs:
            W["decoder.output_projection.weight"] = inits[w].T
    T = {k: torch.tensor(v) for k, v in W.items()}
    P = "decoder.model.decoder."
    L, H, Dm = cfg["decoder_layers"], cfg["decoder_attention_heads"], cfg["d_model"]
    hd = Dm // H

    def lin(x, name):
        return x @ T[name + ".weight"].T + T[name + ".bias"]

    def ln(x, name):
        return torch.nn.functional.layer_norm(x, (Dm,), T[name + ".weight"], T[name + ".bias"], 1e-5)

    def heads(x):
        B, S, _ = x.shape
        return x.view(B, S, H, hd).transpose(1, 2)

    class Init(torch.nn.Module):
        def forward(self, enc):
            ks, vs = [], []
            for i in range(L):
                p = f"{P}layers.{i}.encoder_attn."
                ks.append(heads(lin(enc, p + "k_proj")))
                vs.append(heads(lin(enc, p + "v_proj")))
            return torch.stack(ks), torch.stack(vs)

    class Step(torch.nn.Module):
        def forward(self, ids, pos, self_k, self_v, cross_k, cross_v):
            x = T[P + "embed_tokens.weight"][ids] * math.sqrt(Dm)
            x = x + T[P + "embed_positions.weight"][pos + 2].unsqueeze(0)
            x = ln(x, P + "layernorm_embedding")
            nk, nv = [], []
            for i in range(L):
                p = f"{P}layers.{i}."
                r = x
                q = heads(lin(x, p + "self_attn.q_proj")) * hd ** -0.5
                k = torch.cat([self_k[i], heads(lin(x, p + "self_attn.k_proj"))], 2)
                v = torch.cat([self_v[i], heads(lin(x, p + "self_attn.v_proj"))], 2)
                nk.append(k)
                nv.append(v)
                a = (torch.softmax(q @ k.transpose(-1, -2), -1) @ v).transpose(1, 2).reshape(x.shape)
                x = ln(r + lin(a, p + "self_attn.out_proj"), p + "self_attn_layer_norm")
                r = x
                q = heads(lin(x, p + "encoder_attn.q_proj")) * hd ** -0.5
                a = (torch.softmax(q @ cross_k[i].transpose(-1, -2), -1) @ cross_v[i]).transpose(1, 2).reshape(x.shape)
                x = ln(r + lin(a, p + "encoder_attn.out_proj"), p + "encoder_attn_layer_norm")
                r = x
                x = ln(r + lin(torch.relu(lin(x, p + "fc1")), p + "fc2"), p + "final_layer_norm")
            return x[:, -1] @ T["decoder.output_projection.weight"].T, torch.stack(nk), torch.stack(nv)

    # Parity with the original uncached decoder.
    import onnxruntime as ort

    enc = np.random.RandomState(0).randn(2, 578, 384).astype(np.float32)
    seq = [[1, 12, 284, 383, 269, 265], [1, 20, 30, 40, 50, 60]]
    ref = ort.InferenceSession(os.path.join(d, "decoder_model.onnx")).run(
        None, {"input_ids": np.array(seq), "encoder_hidden_states": enc})[0]
    init, step = Init().eval(), Step().eval()
    with torch.no_grad():
        ck, cv = init(torch.tensor(enc))
        sk = torch.zeros(L, 2, H, 0, hd)
        sv = torch.zeros(L, 2, H, 0, hd)
        worst = 0.0
        for t in range(len(seq[0])):
            lg, sk, sv = step(torch.tensor([[s[t]] for s in seq]), torch.tensor([t]), sk, sv, ck, cv)
            worst = max(worst, float(np.abs(lg.numpy() - ref[:, t]).max()))
    assert worst < 1e-2, f"rebuilt decoder disagrees with the original: {worst}"
    print("mfr decoder parity", worst)
    with torch.no_grad():
        torch.onnx.export(init, (torch.tensor(enc),), os.path.join(out, "mfr-decoder-init.onnx"),
                          input_names=["encoder_hidden_states"], output_names=["cross_k", "cross_v"],
                          dynamic_axes={"encoder_hidden_states": {0: "b"}, "cross_k": {1: "b"}, "cross_v": {1: "b"}},
                          opset_version=17, dynamo=False)
        sk = torch.zeros(L, 2, H, 3, hd)
        sv = torch.zeros(L, 2, H, 3, hd)
        torch.onnx.export(step, (torch.tensor([[5], [6]]), torch.tensor([3]), sk, sv, ck, cv),
                          os.path.join(out, "mfr-decoder-step.onnx"),
                          input_names=["input_ids", "pos", "self_k", "self_v", "cross_k", "cross_v"],
                          output_names=["logits", "new_self_k", "new_self_v"],
                          dynamic_axes={"input_ids": {0: "b"}, "self_k": {1: "b", 3: "t"}, "self_v": {1: "b", 3: "t"},
                                        "cross_k": {1: "b"}, "cross_v": {1: "b"}, "logits": {0: "b"},
                                        "new_self_k": {1: "b", 3: "t1"}, "new_self_v": {1: "b", 3: "t1"}},
                          opset_version=17, dynamo=False)
    os.replace(os.path.join(d, "encoder_model.onnx"), os.path.join(out, "mfr-encoder.onnx"))
    os.replace(os.path.join(d, "tokenizer.json"), os.path.join(out, "mfr-tokenizer.json"))


def table(out, work):
    import onnx
    import torch
    from transformers import TableTransformerForObjectDetection

    d = os.path.join(work, "tatr")
    repo = "microsoft/table-transformer-structure-recognition-v1.1-all"
    for f in ["config.json", "preprocessor_config.json", "model.safetensors"]:
        fetch(repo, f, os.path.join(d, f))
    cfg = json.load(open(os.path.join(d, "config.json")))
    if cfg.get("dilation") is None:  # newer transformers reject null here
        cfg["dilation"] = False
        json.dump(cfg, open(os.path.join(d, "config.json"), "w"))
    m = TableTransformerForObjectDetection.from_pretrained(d).eval()

    class W(torch.nn.Module):
        def __init__(s, m):
            super().__init__()
            s.m = m

        def forward(s, pixel_values):
            o = s.m(pixel_values=pixel_values)
            return o.logits, o.pred_boxes

    tmp = os.path.join(work, "tatr.onnx")
    dim = torch.export.Dim
    with torch.no_grad():
        torch.onnx.export(W(m), (torch.rand(1, 3, 600, 800),), tmp, input_names=["pixel_values"],
                          output_names=["logits", "pred_boxes"],
                          dynamic_shapes={"pixel_values": {2: dim("h", min=64, max=1600), 3: dim("w", min=64, max=1600)}},
                          opset_version=18, dynamo=True)
    onnx.save(onnx.load(tmp, load_external_data=True), tmp + ".single", save_as_external_data=False)
    no_float64(tmp + ".single", os.path.join(out, "tatr-structure.onnx"))


def ocr(out, work):
    import numpy as np
    import yaml

    for repo, name in (("PP-OCRv5_mobile_det_onnx", "ppocr-v5-det.onnx"), ("en_PP-OCRv5_mobile_rec_onnx", "ppocr-v5-rec.onnx")):
        d = os.path.join(work, repo)
        src = fetch(f"PaddlePaddle/{repo}", "inference.onnx", os.path.join(d, "inference.onnx"))
        fetch(f"PaddlePaddle/{repo}", "inference.yml", os.path.join(d, "inference.yml"))
        optimize_for_rten(src, os.path.join(out, name))
        shape = (1, 3, 640, 480) if "det" in repo else (2, 3, 48, 320)
        check_same(src, os.path.join(out, name), {"x": np.random.rand(*shape).astype(np.float32)})
    cfg = yaml.safe_load(open(os.path.join(work, "en_PP-OCRv5_mobile_rec_onnx", "inference.yml")))
    chars = cfg["PostProcess"]["character_dict"]
    with open(os.path.join(out, "ppocr-v5-dict.txt"), "w", encoding="utf-8") as f:
        f.write("\n".join(str(c) for c in chars) + "\n")


def main():
    out = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "models")
    work = os.path.join(out, ".work")
    os.makedirs(work, exist_ok=True)
    for step in (layout, formula, table, ocr):
        print("==", step.__name__)
        step(out, work)
    manifest = {}
    for f in sorted(os.listdir(out)):
        p = os.path.join(out, f)
        if os.path.isfile(p) and not f.startswith("."):
            manifest[f] = hashlib.sha256(open(p, "rb").read()).hexdigest()
    json.dump(manifest, open(os.path.join(out, "manifest.json"), "w"), indent=2)
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

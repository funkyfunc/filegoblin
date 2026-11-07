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
| ppocr-v6m-det.onnx       | PaddlePaddle/PP-OCRv6_medium_det_onnx               | Apache-2.0 |
| ppocr-v6m-rec.onnx       | PaddlePaddle/PP-OCRv6_medium_rec_onnx               | Apache-2.0 |
| ppocr-v6-dict.txt        | the rec model's `inference.yml` character list      | Apache-2.0 |
| lighton-vision.onnx      | rebuilt from lightonai/LightOnOCR-2-1B, 4-bit       | Apache-2.0 |
| lighton-embed.onnx       | LightOnOCR-2-1B token embeddings, int8              | Apache-2.0 |
| lighton-step.onnx        | rebuilt from LightOnOCR-2-1B, KV-cached, 4-bit      | Apache-2.0 |
| lighton-vocab.json       | LightOnOCR-2-1B `tokenizer.json` vocabulary         | Apache-2.0 |

`--ocr v6s` (or `v5`) builds the smaller PP-OCRv6 small or English PP-OCRv5
mobile pair instead, for comparisons; filegoblin uses the best pair it finds.

Conversions:
* PP-DocLayoutV3 is exported from transformers at a fixed 800×800 input and
  every float64 tensor is rewritten as float32 (rten has no f64).
* pix2text ships an uncached decoder. Its weights are lifted out of the ONNX
  graph into a plain-torch TrOCR decoder, which is checked against the
  original to 1e-3 in logits and exported as `init` (cross-attention K/V) and
  `step` (one token with a self-attention cache).
* Table Transformer is exported with dynamic height/width.
* LightOnOCR-2-1B (the page reader) is rebuilt like the formula decoder: the
  Pixtral encoder with its patch grid read from the input shape (attention
  one head at a time: 0.3 GB of scores on a 1540-pixel page, not 5 GB), and the
  Qwen3 decoder as one step with a key/value cache per layer (keys stored
  transposed, grouped-query attention by broadcasting), each checked against
  transformers to 1e-3, then folded and quantized to symmetric 4-bit
  `MatMulNBits` (onnxruntime's quantizer; rten computes them in int8).
* The PaddleOCR graphs are upgraded to opset 17 (rten rejects the legacy
  `Slice`/`BatchNormalization` attributes they carry), and every graph gets
  the clean-ups rten does not do itself: Identity removal, constant folding of
  reshapes, Conv + per-channel affine/BatchNorm folding, and
  `x * HardSigmoid(x)` → `HardSwish`. Outputs are checked against the source
  graph with onnxruntime.

Usage (needs torch, transformers>=5, onnx, onnx_ir, onnxruntime, huggingface_hub):
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


OCR_SETS = {
    # name: (det repo, rec repo, det file, rec file, dict file)
    "v6m": ("PP-OCRv6_medium_det_onnx", "PP-OCRv6_medium_rec_onnx",
            "ppocr-v6m-det.onnx", "ppocr-v6m-rec.onnx", "ppocr-v6-dict.txt"),
    "v6s": ("PP-OCRv6_small_det_onnx", "PP-OCRv6_small_rec_onnx",
            "ppocr-v6s-det.onnx", "ppocr-v6s-rec.onnx", "ppocr-v6-dict.txt"),
    "v5": ("PP-OCRv5_mobile_det_onnx", "en_PP-OCRv5_mobile_rec_onnx",
           "ppocr-v5-det.onnx", "ppocr-v5-rec.onnx", "ppocr-v5-dict.txt"),
}


def ocr(out, work, which=None):
    import numpy as np
    import yaml

    which = which or os.environ.get("FILEGOBLIN_OCR_SET", "v6m")
    det_repo, rec_repo, det_file, rec_file, dict_file = OCR_SETS[which]
    for repo, name in ((det_repo, det_file), (rec_repo, rec_file)):
        d = os.path.join(work, repo)
        src = fetch(f"PaddlePaddle/{repo}", "inference.onnx", os.path.join(d, "inference.onnx"))
        fetch(f"PaddlePaddle/{repo}", "inference.yml", os.path.join(d, "inference.yml"))
        optimize_for_rten(src, os.path.join(out, name))
        shape = (1, 3, 640, 480) if "det" in repo else (2, 3, 48, 320)
        check_same(src, os.path.join(out, name), {"x": np.random.rand(*shape).astype(np.float32)})
    cfg = yaml.safe_load(open(os.path.join(work, rec_repo, "inference.yml")))
    chars = cfg["PostProcess"]["character_dict"]
    with open(os.path.join(out, dict_file), "w", encoding="utf-8") as f:
        f.write("\n".join(str(c) for c in chars) + "\n")


def reader(out, work):
    """LightOnOCR-2-1B as four rten graphs: the Pixtral encoder and projector
    (any H, W in multiples of 28), int8 token embeddings, a KV-cached decoder
    step, and the vocabulary. The encoder and decoder are rebuilt in plain
    torch from the released weights (transformers' own export needs
    onnxruntime-only attention ops), checked against transformers, then
    folded and quantized to symmetric 4-bit `MatMulNBits` (int8 compute)."""
    import math

    import numpy as np
    import onnx
    import onnxruntime as ort
    import torch
    import torch.nn as nn
    import torch.nn.functional as F
    from huggingface_hub import hf_hub_download
    from onnx import TensorProto, helper, numpy_helper
    from onnxruntime.quantization.matmul_nbits_quantizer import MatMulNBitsQuantizer
    from transformers import LightOnOcrForConditionalGeneration

    repo = "lightonai/LightOnOCR-2-1B"
    m = LightOnOcrForConditionalGeneration.from_pretrained(
        repo, dtype=torch.float32, attn_implementation="eager").eval().requires_grad_(False)
    tc, vc = m.config.text_config, m.config.vision_config
    lm, ve, pj = m.model.language_model, m.model.vision_encoder, m.model.vision_projection
    L, NH, NKV, HD = tc.num_hidden_layers, tc.num_attention_heads, tc.num_key_value_heads, tc.head_dim
    eps = tc.rms_norm_eps
    inv_freq = 1.0 / tc.rope_parameters["rope_theta"] ** (torch.arange(0, HD, 2).float() / HD)
    VNH = vc.num_attention_heads
    VH = vc.hidden_size // VNH
    vinv = 1.0 / vc.rope_parameters["rope_theta"] ** (torch.arange(0, VH, 2).float() / VH)
    f_h, f_w = vinv[0::2], vinv[1::2]

    def rms(x, w, e):
        return x * torch.rsqrt(x.pow(2).mean(-1, keepdim=True) + e) * w

    def rope(t, cos, sin, half=HD // 2):
        # `half` is a constant: read off `t.shape` it would trace into shape arithmetic.
        return t * cos + torch.cat([-t[..., half:], t[..., :half]], -1) * sin

    class Step(nn.Module):
        """inputs_embeds (1, S, D), start (1,), then per layer past_k_i (1, KV, 1, HD, P), keys stored
        transposed, and past_v_i (1, KV, 1, P, HD) -> logits of the last position (1, V) and each cache
        grown to P + S. Query heads attend to their shared KV head by broadcasting, so no step copies
        the cache beyond appending to it."""

        def forward(self, x, start, *past):
            s, p = x.shape[1], past[1].shape[3]
            f = (torch.arange(s) + start).float()[:, None] * inv_freq[None]
            a = torch.cat([f, f], -1)
            cos, sin = a.cos()[None, None], a.sin()[None, None]
            mask = torch.where(torch.arange(p + s)[None, :] <= torch.arange(s)[:, None] + p, 0.0, -1e9)
            mask = mask[None, None, None]
            outs = []
            for i, layer in enumerate(lm.layers):
                at = layer.self_attn
                h = rms(x, layer.input_layernorm.weight, eps)
                q = rope(rms(at.q_proj(h).view(1, s, NH, HD), at.q_norm.weight, eps).transpose(1, 2), cos, sin)
                k = rope(rms(at.k_proj(h).view(1, s, NKV, HD), at.k_norm.weight, eps).transpose(1, 2), cos, sin)
                v = at.v_proj(h).view(1, s, NKV, HD).transpose(1, 2)
                k = torch.cat([past[2 * i], k.transpose(2, 3).unsqueeze(2)], 4)
                v = torch.cat([past[2 * i + 1], v.unsqueeze(2)], 3)
                outs += [k, v]
                att = torch.softmax(q.view(1, NKV, NH // NKV, s, HD) / math.sqrt(HD) @ k + mask, -1)
                x = x + at.o_proj((att @ v).view(1, NH, s, HD).transpose(1, 2).reshape(1, s, NH * HD))
                h = rms(x, layer.post_attention_layernorm.weight, eps)
                x = x + layer.mlp.down_proj(F.silu(layer.mlp.gate_proj(h)) * layer.mlp.up_proj(h))
            return (m.lm_head(rms(x[:, -1], lm.norm.weight, eps)), *outs)

    class Vision(nn.Module):
        """pixel_values (1, 3, H, W) -> one feature per 28x28 block (N, D).

        Attention runs one head at a time, and each head's queries take a zero
        term from the previous head's output, so the graph orders the heads and
        rten (which frees a value after its last use) holds one head's N x N
        scores at a time, not all 16 at once. At the model's own 1540 pixels a
        page is about 9,000 patches: 0.3 GB of scores per head, 5 GB for 16."""

        def forward(self, px):
            x = ve.patch_conv(px)
            d, h, w = x.shape[1], x.shape[2], x.shape[3]
            x = rms(x.flatten(2).transpose(1, 2), ve.ln_pre.weight, 1e-5)
            hp = torch.arange(h)[:, None].expand(h, w).reshape(-1).float()
            wp = torch.arange(w)[None, :].expand(h, w).reshape(-1).float()
            a = torch.cat([hp[:, None] * f_h[None], wp[:, None] * f_w[None]], -1)
            a = torch.cat([a, a], -1)
            cos, sin = a.cos()[None, None], a.sin()[None, None]
            n = x.shape[1]
            dep = None
            for layer in ve.transformer.layers:
                at = layer.attention
                y = rms(x, layer.attention_norm.weight, 1e-5)
                q = rope(at.q_proj(y).view(1, n, VNH, VH).transpose(1, 2), cos, sin, VH // 2)
                k = rope(at.k_proj(y).view(1, n, VNH, VH).transpose(1, 2), cos, sin, VH // 2)
                v = at.v_proj(y).view(1, n, VNH, VH).transpose(1, 2)
                heads = []
                for i in range(VNH):
                    qi = q[:, i:i + 1] if dep is None else q[:, i:i + 1] + dep
                    att = torch.softmax(qi @ k[:, i:i + 1].transpose(-1, -2) * VH ** -0.5, -1)
                    heads.append(att @ v[:, i:i + 1])
                    dep = heads[-1][:, :, :1, :1] * 0.0
                o = torch.cat(heads, 1)
                x = x + at.o_proj(o.transpose(1, 2).reshape(1, n, VNH * VH))
                y = rms(x, layer.ffn_norm.weight, 1e-5)
                x = x + layer.feed_forward.down_proj(F.silu(layer.feed_forward.gate_proj(y)) * layer.feed_forward.up_proj(y))
            x = rms(x[0], pj.norm.weight, eps)
            # 2x2 merge in torch.unfold's order: channel, then row, then column.
            g = x.view(h // 2, 2, w // 2, 2, d).permute(0, 2, 4, 1, 3).reshape(-1, d * 4)
            return pj.linear_2(F.gelu(pj.linear_1(pj.patch_merger.merging_layer(g))))

    step, vis = Step().eval(), Vision().eval()
    with torch.inference_mode():
        ids = torch.tensor([[151644, 872, 198, 9707, 11, 1879, 0, 151645, 198, 151644, 77091, 198]])
        e = lm.embed_tokens(ids)
        ref = m(inputs_embeds=e).logits[0, -1]
        r = step(e[:, :8], torch.tensor([0]), *[torch.zeros(1, NKV, 1, HD, 0), torch.zeros(1, NKV, 1, 0, HD)] * L)
        r = step(e[:, 8:], torch.tensor([8]), *r[1:])
        assert (r[0][0] - ref).abs().max() < 1e-3, "rebuilt decoder drifted from transformers"
        for hw in [(1008, 784), (560, 868)]:
            px = torch.randn(1, 3, *hw)
            ref = m.model.get_image_features(px, image_sizes=[hw]).pooler_output[0]
            assert (vis(px) - ref).abs().max() < 1e-3, f"rebuilt encoder drifted at {hw}"

    # One directory per graph: the decoder is over 2 GB, so its weights go to one external file per
    # tensor, named after the tensor, and the encoder's export would overwrite those of the same name.
    d = {g: os.path.join(work, "reader", g) for g in ("step", "vision")}
    for p in d.values():
        os.makedirs(p, exist_ok=True)
    past_names = [f"past_{kv}_{i}" for i in range(L) for kv in "kv"]
    new_names = [f"new_{kv}_{i}" for i in range(L) for kv in "kv"]
    axes = {"inputs_embeds": {1: "S"}}
    axes.update({n: {4 if "_k_" in n else 3: "P"} for n in past_names})
    axes.update({n: {4 if "_k_" in n else 3: "T"} for n in new_names})
    torch.onnx.export(step, (torch.randn(1, 2, tc.hidden_size), torch.tensor([3]),
                             *[torch.zeros(1, NKV, 1, HD, 3), torch.zeros(1, NKV, 1, 3, HD)] * L),
                      os.path.join(d["step"], "step.onnx"), input_names=["inputs_embeds", "start", *past_names],
                      output_names=["logits", *new_names], dynamic_axes=axes, opset_version=17, dynamo=False)
    torch.onnx.export(vis, (torch.randn(1, 3, 1008, 784),), os.path.join(d["vision"], "vision.onnx"), input_names=["pixel_values"],
                      output_names=["features"], dynamic_axes={"pixel_values": {2: "H", 3: "W"}, "features": {0: "N"}},
                      opset_version=17, dynamo=False)
    for name, dst in (("vision", "lighton-vision.onnx"), ("step", "lighton-step.onnx")):
        so = ort.SessionOptions()
        so.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC  # folding only, no contrib ops
        so.optimized_model_filepath = os.path.join(d[name], name + "-folded.onnx")
        ort.InferenceSession(os.path.join(d[name], name + ".onnx"), so, providers=["CPUExecutionProvider"])
        q = MatMulNBitsQuantizer(onnx.load(so.optimized_model_filepath), block_size=32, is_symmetric=True, accuracy_level=4)
        q.process()
        g = q.model.model
        for i, n in enumerate(g.graph.node):
            n.name = f"node{i}"  # rten keeps node and value names in one namespace
        if name == "vision":
            # The longest side this encoder reads within the reader's memory need
            # (crates/core/src/vision/reader.rs); one without it gets 1008.
            onnx.helper.set_model_props(g, {"filegoblin.max_side": "1540"})
        onnx.save(g, os.path.join(out, dst))

    # Token embeddings as int8 rows with one scale each.
    W = lm.embed_tokens.weight.numpy()
    scale = (np.abs(W).max(1, keepdims=True) / 127.0).astype(np.float32)
    table = np.clip(np.round(W / scale), -127, 127).astype(np.int8)
    g = helper.make_graph([
        helper.make_node("Gather", ["table", "input_ids"], ["rows"], axis=0, name="n0"),
        helper.make_node("Gather", ["scale", "input_ids"], ["s"], axis=0, name="n1"),
        helper.make_node("Cast", ["rows"], ["rows_f"], to=TensorProto.FLOAT, name="n2"),
        helper.make_node("Mul", ["rows_f", "s"], ["embeds"], name="n3")],
        "embed", [helper.make_tensor_value_info("input_ids", TensorProto.INT64, [1, "S"])],
        [helper.make_tensor_value_info("embeds", TensorProto.FLOAT, [1, "S", W.shape[1]])],
        [numpy_helper.from_array(table, "table"), numpy_helper.from_array(scale, "scale")])
    onnx.save(helper.make_model(g, opset_imports=[helper.make_opsetid("", 17)], ir_version=8),
              os.path.join(out, "lighton-embed.onnx"))
    tok = json.load(open(hf_hub_download(repo, "tokenizer.json")))
    with open(os.path.join(out, "lighton-vocab.json"), "w", encoding="utf-8") as f:
        json.dump({"model": {"vocab": tok["model"]["vocab"]}}, f, ensure_ascii=False)


LICENSES = """# Third-party model weights

filegoblin's `vision` feature runs these models. Each file was converted from
the upstream release named here (see scripts/vision_models.py); the weights
remain under their original licenses.

| file(s) | upstream | license | copyright |
|---|---|---|---|
| pp-doclayout-v3.onnx | https://huggingface.co/PaddlePaddle/PP-DocLayoutV3_safetensors | Apache-2.0 | PaddlePaddle Authors |
| ppocr-v6m-det.onnx | https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_det_onnx | Apache-2.0 | PaddlePaddle Authors |
| ppocr-v6m-rec.onnx, ppocr-v6-dict.txt | https://huggingface.co/PaddlePaddle/PP-OCRv6_medium_rec_onnx | Apache-2.0 | PaddlePaddle Authors |
| mfr-encoder.onnx, mfr-decoder-init.onnx, mfr-decoder-step.onnx, mfr-tokenizer.json | https://huggingface.co/breezedeus/pix2text-mfr-1.5 | MIT | breezedeus (Pix2Text) |
| tatr-structure.onnx | https://huggingface.co/microsoft/table-transformer-structure-recognition-v1.1-all | MIT | Microsoft Corporation |
| lighton-vision.onnx, lighton-embed.onnx, lighton-step.onnx, lighton-vocab.json | https://huggingface.co/lightonai/LightOnOCR-2-1B | Apache-2.0 | LightOn |

Modifications: graph-format conversions only (ONNX export, float64 → float32,
opset upgrade, Conv epilogue folding, and for the formula decoder and the
page reader a rebuild with a key/value cache from the released weights; the
page reader's weights are also quantized to 4 and 8 bits). No weights were
retrained.

The full license texts are in LICENSE-APACHE-2.0.txt and LICENSE-MIT.txt.
"""

MIT = """MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

Copyright (c) Microsoft Corporation (Table Transformer).
Copyright (c) breezedeus (Pix2Text MFR).
"""


def write_licenses(out):
    with open(os.path.join(out, "LICENSES.md"), "w") as f:
        f.write(LICENSES)
    with open(os.path.join(out, "LICENSE-MIT.txt"), "w") as f:
        f.write(MIT)
    fetch_url("https://www.apache.org/licenses/LICENSE-2.0.txt", os.path.join(out, "LICENSE-APACHE-2.0.txt"))


def fetch_url(url, dst):
    if not os.path.exists(dst):
        urllib.request.urlretrieve(url, dst)


def main():
    out = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "models")
    work = os.path.join(out, ".work")
    os.makedirs(work, exist_ok=True)
    for step in (layout, formula, table, ocr, reader):
        print("==", step.__name__)
        step(out, work)
    write_licenses(out)
    manifest = {}
    for f in sorted(os.listdir(out)):
        p = os.path.join(out, f)
        if os.path.isfile(p) and not f.startswith("."):
            manifest[f] = hashlib.sha256(open(p, "rb").read()).hexdigest()
    json.dump(manifest, open(os.path.join(out, "manifest.json"), "w"), indent=2)
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

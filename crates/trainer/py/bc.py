"""Behaviour cloning for the policy brain, numpy only.

Reads the `policy-train extract` files (PREFIX.x.f16, PREFIX.rows.f32, PREFIX.groups.u32;
several prefixes may be given), and fits score = lin.x + w2.relu(W1 x + b1) + b2 so that a
softmax over each tick's candidates (plus the sampling log-weights) puts the teacher's
choice first. Games are split into train and validation sets. Writes the model JSON the
Rust brain reads.

usage: python3 bc.py OUT.json PREFIX [PREFIX...] [--hidden 32] [--epochs 30] [--lr 3e-3]
"""

import json
import sys
import time

import numpy as np

F = None
sys_features = 75


def load(prefix):
    g = np.fromfile(prefix + ".groups.u32", dtype=np.uint32)
    g = g[: g.size // 5 * 5].reshape(-1, 5).astype(np.int64)
    rows = np.fromfile(prefix + ".rows.f32", dtype=np.float32)
    rows = rows[: rows.size // 2 * 2].reshape(-1, 2)
    x = np.fromfile(prefix + ".x.f16", dtype=np.float16)
    f = int(sys_features)
    n = min(rows.shape[0], x.size // f)
    rows, x = rows[:n], x[: n * f]
    g = g[g[:, 0] + g[:, 1] <= n]
    return g, rows, x.reshape(-1, f), f


def main():
    args = sys.argv[1:]
    opts = {"--hidden": 32, "--epochs": 30, "--lr": 3e-3, "--l2": 1e-5, "--batch": 256, "--seed": 1, "--val": 0.15, "--init": ""}
    pos = []
    i = 0
    while i < len(args):
        if args[i] in opts:
            t = type(opts[args[i]])
            opts[args[i]] = t(args[i + 1])
            i += 2
        else:
            pos.append(args[i])
            i += 1
    out, prefixes = pos[0], pos[1:]
    gs, rs, xs = [], [], []
    base = 0
    game_base = 0
    for p in prefixes:
        g, r, x, f = load(p)
        g = g.copy()
        g[:, 0] += base
        g[:, 4] += game_base
        game_base = g[:, 4].max() + 1
        base += r.shape[0]
        gs.append(g)
        rs.append(r)
        xs.append(x)
    G = np.concatenate(gs)
    R = np.concatenate(rs)
    X = np.concatenate(xs).astype(np.float32)
    nf = X.shape[1]
    print(f"{G.shape[0]} samples, {X.shape[0]} rows, {nf} features, {len(set(G[:,4]))} games", flush=True)

    mean = X.mean(0)
    std = X.std(0)
    inv = np.where(std > 1e-6, 1.0 / np.maximum(std, 1e-6), 0.0).astype(np.float32)
    Xn = (X - mean) * inv
    corr = R[:, 0].astype(np.float32)
    kind = R[:, 1].astype(np.int64)

    rng = np.random.default_rng(opts["--seed"])
    games = np.unique(G[:, 4])
    rng.shuffle(games)
    nval = max(1, int(len(games) * opts["--val"]))
    val_games = set(games[:nval].tolist())
    is_val = np.array([g in val_games for g in G[:, 4]])
    tr_idx = np.where(~is_val)[0]
    va_idx = np.where(is_val)[0]

    H = opts["--hidden"]
    if opts["--init"]:
        m = json.load(open(opts["--init"]))
        lin = np.array(m["lin"], np.float32)
        W1 = np.array(m["w1"], np.float32).reshape(H, nf)
        b1 = np.array(m["b1"], np.float32)
        w2 = np.array(m["w2"], np.float32)
        b2 = np.float32(m["b2"])
    else:
        lin = np.zeros(nf, np.float32)
        W1 = (rng.standard_normal((H, nf)) * np.sqrt(2.0 / nf) * 0.5).astype(np.float32)
        b1 = np.zeros(H, np.float32)
        w2 = (rng.standard_normal(H) * 0.1).astype(np.float32)
        b2 = np.float32(0.0)
    params = [lin, W1, b1, w2]
    mom = [np.zeros_like(p) for p in params]
    vel = [np.zeros_like(p) for p in params]
    beta1, beta2, eps = 0.9, 0.999, 1e-8
    step = 0

    def gather_rows(idx):
        starts = G[idx, 0]
        lens = G[idx, 1]
        rows = np.concatenate([np.arange(s, s + n) for s, n in zip(starts, lens)])
        seg = np.repeat(np.arange(len(idx)), lens)
        offs = np.concatenate([[0], np.cumsum(lens)[:-1]])
        return rows, seg, offs, lens

    def forward(rows):
        x = Xn[rows]
        pre = x @ W1.T + b1
        h = np.maximum(pre, 0)
        s = x @ lin + h @ w2 + b2
        return x, pre, h, s

    def group_softmax(s, seg, offs, n):
        z = s + corr_rows
        mx = np.maximum.reduceat(z, offs)
        e = np.exp(z - mx[seg])
        tot = np.add.reduceat(e, offs)
        p = e / tot[seg]
        nll = -(z[offs] - mx - np.log(tot))
        return p, nll

    def evaluate(idx):
        nonlocal corr_rows
        tot_nll, acc, n = 0.0, 0, 0
        acc_k = np.zeros(9)
        cnt_k = np.zeros(9)
        for b in range(0, len(idx), 2048):
            bi = idx[b:b + 2048]
            rows, seg, offs, lens = gather_rows(bi)
            corr_rows = corr[rows]
            _, _, _, s = forward(rows)
            p, nll = group_softmax(s, seg, offs, len(bi))
            tot_nll += nll.sum()
            mxs = np.maximum.reduceat(s, offs)
            hit = s[offs] >= mxs
            acc += hit.sum()
            k = kind[rows[offs]]
            np.add.at(acc_k, k, hit)
            np.add.at(cnt_k, k, 1)
            n += len(bi)
        return tot_nll / n, acc / n, acc_k / np.maximum(cnt_k, 1), cnt_k

    corr_rows = None
    lr = opts["--lr"]
    bs = opts["--batch"]
    best = (1e9, None)
    t0 = time.time()
    for ep in range(opts["--epochs"]):
        rng.shuffle(tr_idx)
        cur_lr = lr * (0.5 * (1 + np.cos(np.pi * ep / opts["--epochs"])))
        for b in range(0, len(tr_idx), bs):
            bi = tr_idx[b:b + bs]
            rows, seg, offs, lens = gather_rows(bi)
            corr_rows = corr[rows]
            x, pre, h, s = forward(rows)
            p, nll = group_softmax(s, seg, offs, len(bi))
            ds = p.copy()
            ds[offs] -= 1.0
            ds /= len(bi)
            g_lin = x.T @ ds + opts["--l2"] * lin
            g_w2 = h.T @ ds + opts["--l2"] * w2
            dh = np.outer(ds, w2) * (pre > 0)
            g_W1 = dh.T @ x + opts["--l2"] * W1
            g_b1 = dh.sum(0)
            grads = [g_lin, g_W1, g_b1, g_w2]
            step += 1
            for k, (pp, gg) in enumerate(zip(params, grads)):
                mom[k] = beta1 * mom[k] + (1 - beta1) * gg
                vel[k] = beta2 * vel[k] + (1 - beta2) * gg * gg
                mh = mom[k] / (1 - beta1 ** step)
                vh = vel[k] / (1 - beta2 ** step)
                pp -= (cur_lr * mh / (np.sqrt(vh) + eps)).astype(np.float32)
        tr = evaluate(tr_idx[:20000])
        va = evaluate(va_idx)
        print(f"epoch {ep}: train nll {tr[0]:.3f} acc {tr[1]:.3f} | val nll {va[0]:.3f} acc {va[1]:.3f} ({time.time()-t0:.0f}s)", flush=True)
        if va[0] < best[0]:
            best = (va[0], [p.copy() for p in params], va)
    lin_b, W1_b, b1_b, w2_b = best[1]
    va = best[2]
    names = ["capN", "capE", "relay", "farm", "village", "quarry", "wall", "gather", "noop"]
    print("best val nll %.3f acc %.3f" % (va[0], va[1]))
    print("per-kind val acc:", {n: (round(float(a), 3), int(c)) for n, a, c in zip(names, va[2], va[3])})
    model = {
        "f": int(nf), "h": int(H),
        "mean": mean.astype(float).tolist(), "inv_std": inv.astype(float).tolist(),
        "lin": lin_b.astype(float).tolist(), "w1": W1_b.reshape(-1).astype(float).tolist(),
        "b1": b1_b.astype(float).tolist(), "w2": w2_b.astype(float).tolist(), "b2": float(b2),
    }
    json.dump(model, open(out, "w"))
    print("wrote", out)


if __name__ == "__main__":
    main()

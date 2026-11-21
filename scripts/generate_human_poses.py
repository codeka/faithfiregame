"""Generates world/assets/characters/human_poses.glb with static death/incapacitated pose clips."""

import json
import math
import pathlib
import struct

ROOT = pathlib.Path(__file__).resolve().parents[1]
BASE_BODY_PATH = ROOT / "world" / "assets" / "characters" / "human_base_body.glb"
RIFLE_PATH = ROOT / "world" / "assets" / "characters" / "weapon_guard_rifle.glb"
OUT_PATH = ROOT / "world" / "assets" / "characters" / "human_poses.glb"


def read_glb(path: pathlib.Path):
    raw = path.read_bytes()
    jlen = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20 : 20 + jlen].decode("utf-8"))
    bin_off = 20 + jlen
    blob = raw[bin_off + 8 :] if bin_off + 8 <= len(raw) else b""
    return doc, blob


def euler_xyz_to_quat(rx: float, ry: float, rz: float):
    # Matches standard YXZ/XYZ? Build explicitly: q = qz * qy * qx (intrinsic XYZ / extrinsic XYZ)
    cx, sx = math.cos(rx * 0.5), math.sin(rx * 0.5)
    cy, sy = math.cos(ry * 0.5), math.sin(ry * 0.5)
    cz, sz = math.cos(rz * 0.5), math.sin(rz * 0.5)
    qx = (sx, 0.0, 0.0, cx)
    qy = (0.0, sy, 0.0, cy)
    qz = (0.0, 0.0, sz, cz)
    return qmul(qz, qmul(qy, qx))


def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


def qrot_vec(q, v):
    qx, qy, qz, qw = q
    vx, vy, vz = v
    tx = 2.0 * (qy * vz - qz * vy)
    ty = 2.0 * (qz * vx - qx * vz)
    tz = 2.0 * (qx * vy - qy * vx)
    return (
        vx + qw * tx + (qy * tz - qz * ty),
        vy + qw * ty + (qz * tx - qx * tz),
        vz + qw * tz + (qx * ty - qy * tx),
    )


def build_pose_specs():
    return {
        "DeadSlumped": {
            "translations": {
                "pelvis": (0.0, 0.18, -0.14),
            },
            "rotations": {
                "pelvis": euler_xyz_to_quat(-0.34, 0.0, 0.03),
                "spine_01": euler_xyz_to_quat(0.16, 0.0, 0.04),
                "spine_02": euler_xyz_to_quat(0.22, -0.05, -0.06),
                "neck": euler_xyz_to_quat(0.26, 0.06, -0.10),
                "head": euler_xyz_to_quat(0.38, 0.14, -0.18),
                "clavicle_l": euler_xyz_to_quat(0.0, 0.0, -0.12),
                "upperarm_l": euler_xyz_to_quat(-0.22, 0.0, 0.24),
                "lowerarm_l": euler_xyz_to_quat(-0.52, 0.0, -0.12),
                "hand_l": euler_xyz_to_quat(0.10, 0.0, 0.0),
                "clavicle_r": euler_xyz_to_quat(0.0, 0.0, 0.12),
                "upperarm_r": euler_xyz_to_quat(-0.46, -0.10, -0.28),
                "lowerarm_r": euler_xyz_to_quat(-0.88, -0.10, 0.10),
                "hand_r": euler_xyz_to_quat(-0.10, -0.28, 0.12),
                "thigh_l": euler_xyz_to_quat(-1.28, 0.14, 0.08),
                "calf_l": euler_xyz_to_quat(0.18, 0.0, 0.0),
                "foot_l": euler_xyz_to_quat(-0.22, 0.0, 0.0),
                "thigh_r": euler_xyz_to_quat(-1.32, -0.16, -0.10),
                "calf_r": euler_xyz_to_quat(0.24, 0.0, 0.0),
                "foot_r": euler_xyz_to_quat(-0.24, 0.0, 0.0),
            },
        },
        "DeadSupine": {
            "translations": {
                "pelvis": (0.0, 0.14, 0.0),
            },
            "rotations": {
                "pelvis": euler_xyz_to_quat(-1.54, 0.0, 0.04),
                "spine_01": euler_xyz_to_quat(0.03, 0.0, -0.02),
                "spine_02": euler_xyz_to_quat(0.04, 0.03, -0.03),
                "neck": euler_xyz_to_quat(0.06, 0.18, 0.05),
                "head": euler_xyz_to_quat(0.08, 0.48, 0.10),
                "clavicle_l": euler_xyz_to_quat(0.0, 0.0, -0.05),
                "upperarm_l": euler_xyz_to_quat(0.04, 0.0, 0.36),
                "lowerarm_l": euler_xyz_to_quat(-0.26, 0.0, 0.10),
                "clavicle_r": euler_xyz_to_quat(0.0, 0.0, 0.05),
                "upperarm_r": euler_xyz_to_quat(0.05, 0.0, -0.46),
                "lowerarm_r": euler_xyz_to_quat(-0.34, 0.0, -0.14),
                "thigh_l": euler_xyz_to_quat(0.04, 0.0, 0.16),
                "calf_l": euler_xyz_to_quat(0.14, 0.0, 0.0),
                "foot_l": euler_xyz_to_quat(-0.18, 0.0, 0.0),
                "thigh_r": euler_xyz_to_quat(0.02, 0.0, -0.14),
                "calf_r": euler_xyz_to_quat(0.08, 0.0, 0.0),
                "foot_r": euler_xyz_to_quat(-0.14, 0.0, 0.0),
            },
        },
        "DeadProne": {
            "translations": {
                "pelvis": (0.0, 0.14, 0.0),
            },
            "rotations": {
                "pelvis": euler_xyz_to_quat(1.54, 0.0, -0.04),
                "spine_01": euler_xyz_to_quat(-0.03, 0.0, 0.02),
                "spine_02": euler_xyz_to_quat(-0.04, -0.03, 0.03),
                "neck": euler_xyz_to_quat(-0.08, 0.25, 0.0),
                "head": euler_xyz_to_quat(-0.10, 0.85, -0.08),
                "upperarm_l": euler_xyz_to_quat(-0.12, 0.0, 0.42),
                "lowerarm_l": euler_xyz_to_quat(0.32, 0.0, 0.15),
                "upperarm_r": euler_xyz_to_quat(-0.06, 0.0, -0.32),
                "lowerarm_r": euler_xyz_to_quat(0.20, 0.0, -0.10),
                "thigh_l": euler_xyz_to_quat(0.06, 0.0, 0.14),
                "calf_l": euler_xyz_to_quat(0.22, 0.0, 0.0),
                "foot_l": euler_xyz_to_quat(-0.20, 0.0, 0.0),
                "thigh_r": euler_xyz_to_quat(0.04, 0.0, -0.18),
                "calf_r": euler_xyz_to_quat(0.12, 0.0, 0.0),
                "foot_r": euler_xyz_to_quat(-0.14, 0.0, 0.0),
            },
        },
    }


def evaluate_fk(nodes, pose_spec):
    parent_of = {}
    for idx, n in enumerate(nodes):
        for c in n.get("children", []):
            parent_of[c] = idx

    world_pos = [None] * len(nodes)
    world_rot = [None] * len(nodes)

    def eval_node(i):
        if world_pos[i] is not None:
            return
        n = nodes[i]
        name = n.get("name", "")
        lt = pose_spec["translations"].get(name, tuple(n.get("translation", [0.0, 0.0, 0.0])))
        lr = pose_spec["rotations"].get(name, tuple(n.get("rotation", [0.0, 0.0, 0.0, 1.0])))
        p = parent_of.get(i)
        if p is None:
            world_pos[i] = lt
            world_rot[i] = lr
        else:
            eval_node(p)
            pt, pr = world_pos[p], world_rot[p]
            rt = qrot_vec(pr, lt)
            world_pos[i] = (pt[0] + rt[0], pt[1] + rt[1], pt[2] + rt[2])
            world_rot[i] = qmul(pr, lr)

    for idx in range(len(nodes)):
        eval_node(idx)
    return {nodes[i].get("name", f"n{i}"): (world_pos[i], world_rot[i]) for i in range(len(nodes))}


def main():
    base_doc, _ = read_glb(BASE_BODY_PATH)
    rifle_doc, _ = read_glb(RIFLE_PATH)
    base_nodes = base_doc["nodes"]

    # Extract skeleton nodes only (joints 1..20 + root hierarchy; keep indices 0..19 for bone nodes)
    joint_indices = base_doc["skins"][0]["joints"]
    joint_set = set(joint_indices)
    old_to_new = {old: new for new, old in enumerate(joint_indices)}
    out_nodes = []
    for old in joint_indices:
        src = base_nodes[old]
        node = {"name": src["name"]}
        if "translation" in src:
            node["translation"] = src["translation"]
        if "rotation" in src:
            node["rotation"] = src["rotation"]
        children = [old_to_new[c] for c in src.get("children", []) if c in joint_set]
        if children:
            node["children"] = children
        out_nodes.append(node)

    node_by_name = {n["name"]: idx for idx, n in enumerate(out_nodes)}
    pose_specs = build_pose_specs()

    # Validate ground clearance with full hierarchy (including sockets) and rifle AABB
    rifle_mins = [a["min"] for a in rifle_doc["accessors"] if "min" in a]
    rifle_maxs = [a["max"] for a in rifle_doc["accessors"] if "max" in a]
    r_min = [min(m[k] for m in rifle_mins) for k in range(3)]
    r_max = [max(m[k] for m in rifle_maxs) for k in range(3)]

    for clip_name, spec in pose_specs.items():
        fk = evaluate_fk(base_nodes, spec)
        joint_ys = [fk[out_nodes[i]["name"]][0][1] for i in range(len(out_nodes))]
        min_y = min(joint_ys[1:])  # skip root at 0
        max_y = max(joint_ys)
        print(f"{clip_name}: joint y in [{min_y:.3f}, {max_y:.3f}] (head={fk['head'][0][1]:.3f}, foot_l={fk['foot_l'][0][1]:.3f}, foot_r={fk['foot_r'][0][1]:.3f}, hand_r={fk['hand_r'][0][1]:.3f})")
        if clip_name == "DeadSlumped":
            sock_p, sock_r = fk["socket_hand_r"]
            corners_y = []
            for cx in (r_min[0], r_max[0]):
                for cy in (r_min[1], r_max[1]):
                    for cz in (r_min[2], r_max[2]):
                        rv = qrot_vec(sock_r, (cx, cy, cz))
                        corners_y.append(sock_p[1] + rv[1])
            print(f"  socket_hand_r={sock_p}, rifle world y in [{min(corners_y):.3f}, {max(corners_y):.3f}]")

    # Pack binary buffer for GLB animations (2 identical keyframes at t=0.0, t=0.5 per channel)
    bin_buf = bytearray()
    buffer_views = []
    accessors = []

    def add_accessor(floats, acc_type, with_minmax=False):
        off = len(bin_buf)
        for v in floats:
            bin_buf.extend(struct.pack("<f", float(v)))
        bv_idx = len(buffer_views)
        buffer_views.append({"buffer": 0, "byteOffset": off, "byteLength": len(bin_buf) - off})
        count = len(floats) // (1 if acc_type == "SCALAR" else (3 if acc_type == "VEC3" else 4))
        acc = {
            "bufferView": bv_idx,
            "componentType": 5126,
            "count": count,
            "type": acc_type,
        }
        if with_minmax and acc_type == "SCALAR":
            acc["min"] = [float(min(floats))]
            acc["max"] = [float(max(floats))]
        idx = len(accessors)
        accessors.append(acc)
        return idx

    time_acc = add_accessor([0.0, 0.5], "SCALAR", with_minmax=True)
    animations = []

    for clip_name, spec in pose_specs.items():
        samplers = []
        channels = []
        for bone_name, t_val in spec["translations"].items():
            out_acc = add_accessor(list(t_val) + list(t_val), "VEC3")
            samp_idx = len(samplers)
            samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
            channels.append({
                "sampler": samp_idx,
                "target": {"node": node_by_name[bone_name], "path": "translation"},
            })
        for bone_name, q_val in spec["rotations"].items():
            out_acc = add_accessor(list(q_val) + list(q_val), "VEC4")
            samp_idx = len(samplers)
            samplers.append({"input": time_acc, "output": out_acc, "interpolation": "LINEAR"})
            channels.append({
                "sampler": samp_idx,
                "target": {"node": node_by_name[bone_name], "path": "rotation"},
            })
        animations.append({"name": clip_name, "samplers": samplers, "channels": channels})

    while len(bin_buf) % 4 != 0:
        bin_buf.append(0)

    gltf_doc = {
        "asset": {"version": "2.0", "generator": "Faith & Fire generate_human_poses.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": out_nodes,
        "animations": animations,
        "buffers": [{"byteLength": len(bin_buf)}],
        "bufferViews": buffer_views,
        "accessors": accessors,
    }

    json_bytes = json.dumps(gltf_doc, separators=(",", ":")).encode("utf-8")
    while len(json_bytes) % 4 != 0:
        json_bytes += b" "

    total_len = 12 + 8 + len(json_bytes) + 8 + len(bin_buf)
    header = struct.pack("<3I", 0x46546C67, 2, total_len)
    json_chunk = struct.pack("<2I", len(json_bytes), 0x4E4F534A) + json_bytes
    bin_chunk = struct.pack("<2I", len(bin_buf), 0x004E4942) + bytes(bin_buf)
    OUT_PATH.write_bytes(header + json_chunk + bin_chunk)
    print(f"Wrote {OUT_PATH} ({total_len} bytes, {len(animations)} clips)")


if __name__ == "__main__":
    main()

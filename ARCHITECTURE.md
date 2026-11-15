# Faith & Fire — Technical Architecture & Engineering Specification

> **Purpose:** Engineering blueprint and project conventions for **Faith & Fire**, a browser-based 3D third-person online RPG. Drop this file into your repository root (or `.agents/rules/`) so AI coding agents have full architectural context.

---

## 1. High-Level Overview & Gameplay Loop
- **Title:** Faith & Fire
- **Genre:** Online Role-Playing Game (MMORPG / Online RPG)
- **Client Platform:** Web Browser (HTML5 Canvas + WebGL2 + DOM UI overlay)
- **Camera & Controls:** 3D, Third-Person, Point-and-Click:
  - **Loose Follow Camera:** Centered loosely on the player; **scroll wheel** zooms in/out (clamped near active simulation range, with distance fog masking any unsimulated edges) and **middle-click + drag** rotates/orbits the view.
  - Click terrain/ground to pathfind and move (with cursor feedback on impassable cells and optional client-side ghost path previews).
  - Click enemies to target and engage in combat.
  - Click other players and NPCs to initiate dialogue, trade, and world interactions.

---

## 2. Technology Stack

| Layer | Technology | Notes |
| :--- | :--- | :--- |
| **Authoritative Server** | **Rust** (`tokio`, `axum` / `tokio-tungstenite`) | Single lightweight binary hosting WebSocket game sessions, HTTP auth, and static world/asset HTTP(S) endpoints; optimized for low-powered VPS hosting. |
| **Client 3D & Core Logic** | **Rust → WebAssembly (WASM)** (`wasm-bindgen`, `web-sys` / `glow`) | Renders 3D scene to full-viewport `<canvas>` via WebGL2; handles interpolation, input picking, and client prediction. |
| **Client 2D UI** | **Angular (TypeScript)** | Overlaid on top of the WebGL canvas (`pointer-events: none` root wrapper; `pointer-events: auto` on interactive HUD/windows). |
| **Shared Logic & Protocol** | **Pure Rust (`shared` crate)** | Compiled into both `server` and `client-wasm`. Holds wire packet enums, world binary schemas, and 1m grid pathfinding/LOS. |
| **Networking** | **Binary WebSockets + `postcard`** | Persistent bidirectional WebSocket connection carrying `postcard`-serialized (`serde`) binary frames. |
| **Persistence** | **SQLite** (`sqlx` in WAL mode) | Zero-config single-file DB, abstracted behind a Rust repository trait for future migration to PostgreSQL. |
| **3D Assets** | **glTF 2.0 / GLB (`.glb`)** | Used for terrain props, characters, enemies, and interactive objects. |

---

## 3. Recommended Repository & Cargo Workspace Layout

```text
faith-and-fire/
├── Cargo.toml                  # Virtual workspace manifest
├── ARCHITECTURE.md             # This technical specification
├── crates/
│   ├── shared/                 # Pure Rust (compiles to native & wasm32-unknown-unknown)
│   │   └── src/
│   │       ├── protocol.rs     # ClientMessage, ServerMessage, PROTOCOL_VERSION
│   │       ├── world/          # walk_mask.rs, types.rs, outside.rs, inside.rs
│   │       └── pathfinding.rs  # 1m grid A* + Line-of-Sight (Theta* / string-pulling)
│   ├── server/                 # Authoritative Rust game server
│   │   └── src/
│   │       ├── main.rs
│   │       ├── seed/           # mesh_builder.rs, decorations.rs, world_seed.rs
│   │       ├── sim/            # world.rs (10Hz loop, 3x3 hibernation), player.rs
│   │       ├── net/            # state.rs, auth.rs, world_files.rs, ws.rs
│   │       ├── db/             # repository.rs (Storage trait), sqlite.rs (SQLite WAL)
│   │       └── editor_api/     # #[cfg(feature = "editor")] areas.rs, save.rs
│   └── client-wasm/            # Rust WASM 3D engine & game client core
│       └── src/
│           ├── lib.rs          # #[wasm_bindgen] exports & Angular state bridge
│           ├── engine.rs       # EngineInner state & frame loop
│           ├── input/          # state.rs, dom.rs (keyboard/mouse listeners)
│           ├── camera/         # projection.rs, controller.rs, gameplay_camera.rs, editor_camera.rs
│           ├── render/         # renderer.rs, shaders.rs, shadow_map.rs, terrain_mesh.rs, prop_mesh.rs, procedural_textures.rs
│           ├── glb_loader.rs   # Binary .glb scene-graph mesh parser
│           ├── net/            # ws_client.rs, http.rs
│           └── editor/         # #[cfg(feature = "editor")] state.rs, types.rs, terrain_ops.rs,
│                               # heightmap_tool.rs, texture_tool.rs, decoration_tool.rs, scene_render.rs, io.rs
├── scripts/                    # Tooling scripts (build-wasm.ps1, backup-world.ps1)
├── web/                        # Angular (TypeScript) frontend shell
│   └── src/
│       └── app/
│           ├── game-canvas/    # Hosts <canvas> and initializes WASM module
│           ├── hud/            # Action bars, target frame, inventory, chat, trade modals
│           └── editor-ui/      # Dev-only Angular editor panels (stripped in prod builds)
└── world/                      # Gitignored runtime/authored world data (seeded on boot; backed up via scripts/backup-world.ps1)
    ├── assets/                 # Shared .glb models and terrain textures
    └── areas/
        ├── <outside_area>/
        │   ├── area.bin        # AreaMetadata (Outside)
        │   └── <x>,<y>/
        │       ├── sim.bin     # TileSimData (Server authoritative)
        │       └── vis.bin     # TileVisData (Client visual)
        └── <inside_area>/
            ├── area.bin        # AreaMetadata (Inside: Generated | Ephemeral | Permanent)
            ├── sim.bin
            └── vis.bin
```

> **Rust Module Convention:** `mod.rs` files strictly declare submodules and `pub use` re-exports; domain types and implementation logic live in focused, single-purpose submodule files.

---

## 4. World Spatial Partitioning ("Areas")

The world is partitioned into distinct **Areas** defined by simulation architecture rather than visual ceiling/skybox appearance (e.g., a massive cavern may be modeled as an **Outside** area):

### A. Outside Areas
- **Grid Structure:** Effectively infinite/vast open spaces divided into uniform **64m × 64m tiles** indexed by `(x, y)` coordinates.
- **Demand-Driven Simulation (Hibernation):** Unoccupied regions consume zero CPU and remain unloaded. For every online player in an Outside area, the server keeps a configurable **`3×3` window of tiles** (9 tiles total; tunable e.g. to `5×5` based on playtesting) centered on the player's current tile active and simulated. Overlapping windows share simulation state; tiles leaving all active windows are flushed/unloaded.
- **Client View & Fog:** Clients render tiles within view and tune camera zoom bounds so players rarely zoom out past active simulation range; distance fog masks unsimulated boundaries.
- **Persistence:** Static and permanent world geometry.

### B. Inside Areas
- **Unit Structure:** Bounded spaces with **arbitrary `(width × length)` dimensions** simulated as a single, self-contained unit rather than a fixed `64m × 64m` tile grid. Only simulated while at least one player is inside.
- **Sub-types & Lifecycle:**
  1. **Generated Instances (e.g., Dungeons / Mazes):** Assembled dynamically on-demand from modular `.glb` pieces when a player/party enters, yielding a unique layout each run; layout/props are sent to clients via `ServerMessage` (referencing HTTP(S)-served `.glb` assets) and destroyed after exit.
  2. **Ephemeral / Resetting Instances (e.g., Shops):** Fixed-layout single-unit areas that reset to their default state once all players leave.
  3. **Permanent Instances (e.g., Player Homes):** Fixed-layout persistent spaces whose modified state is saved to SQLite and restored on subsequent visits.

---

## 5. Simulation Loop, Pathfinding & Combat Rules

- **Server Tick Rate:** **10 Hz** (`100ms` per tick) across all active Outside tiles and active Inside instances.
- **1m × 1m Navigation Grid:**
  - Every 64m × 64m tile is subdivided into **64 × 64 cells (1m × 1m per cell)**.
  - Walkability is stored as a compact bitmask (`[u64; 64]` = 512 bytes per tile).
  - **Path Smoothing:** Pathfinding runs on the 1m grid, then applies Line-of-Sight (LOS) string-pulling / Theta* checks so entities move along direct straight-line segments from Point A to Point B whenever unobstructed.
- **Instant Projectile Resolution:**
  - Projectiles (arrows, spells) have **no server-side flight physics**. Hit/miss, LOS, and damage are resolved immediately at fire time on the server tick; the client animates the visual projectile trajectory between shooter and target.
  - **Cross-Tile Ranged Reach:** Ranged attacks can query and hit targets in immediately adjacent neighboring tiles (1-tile boundary reach). Multi-tile long-range projectiles are out of scope for v1.

---

## 6. Networking & Serialization Protocol

- **Authentication:** Clients authenticate via an HTTP login endpoint (`axum`) that issues a session token, which is then passed over the WebSocket during handshake/session initialization.
- **Wire Format:** Binary WebSocket frames encoded with `postcard` (`serde`).
- **Handshake:** Upon WebSocket open, the client immediately sends `ClientMessage::Handshake { protocol_version: u32, session_token }` (or authenticates with the session token right after version validation). If mismatched with `shared::PROTOCOL_VERSION`, the server responds with `ServerMessage::VersionMismatch` and closes cleanly.
- **World & Asset Transport:** Static `vis.bin` chunks and `.glb` models are served over standard HTTP(S) endpoints (`axum`) to leverage browser caching, while dynamically generated indoor layouts are sent over WebSocket via `ServerMessage` referencing those HTTP(S) `.glb` assets.
- **Debugging & Inspection:**
  - All `ClientMessage` and `ServerMessage` enums derive `Debug` (and `Serialize`/`Deserialize`).
  - Support packet tracing via `tracing` on the server (`RUST_LOG=...=trace`) and a dev-only console logger toggle in WASM.
  - Optional `json-wire` Cargo feature in `shared` can swap `postcard` for `serde_json` during local debugging in browser DevTools.

---

## 7. Split World Storage Format (`sim.bin` & `vis.bin`)

World files are serialized with `postcard` and split so the server never loads visual-only assets and clients only download visual data for tiles in view:

- **`sim.bin` (Server Authoritative):**
  - `walkable: [u64; 64]` — 64×64 walkability bitmask (1m grid).
  - `elevation: Vec<f32>` (or quantized heights) — Height samples for LOS/range elevation validation.
  - `triggers: Vec<TriggerVolume>` — Portals, area transitions, interaction bounds.
  - `spawns: Vec<EntitySpawn>` — NPC and enemy spawn points, templates, and leash bounds.
- **`vis.bin` (Client Visual):**
  - `walkable: [u64; 64]` — Shared 64×64 walkability bitmask (512 bytes) for impassable cursor feedback, optional client-side ghost path previews, and editor walkability overlays.
  - `heights: Vec<f32>` — **65 × 65** heightmap vertices (4,225 floats) so adjacent 64m tiles share border vertices without terrain seams.
  - `splat_map: Vec<u32>` — 64 × 64 terrain texture blend weights/indices.
  - `props: Vec<PropPlacement>` — Static `.glb` prop references (`asset_id`, `position`, `rotation`, `scale`).
- **Inside Area Schemas:** Fixed Inside areas (`Ephemeral` / `Permanent`) store arbitrary `(width, length)` walkability/elevation grids and prop placements (with terrain heightmaps/splatmaps optional when interiors are built purely from `.glb` meshes). **Generated** Inside areas transmit their assembled layout, `.glb` piece placements, and walkability grid to the client via `ServerMessage` upon entry.

---

## 8. Integrated World Editor & Release Build Gating

- **In-Engine Editor:** Built directly into the `client-wasm` + Angular frontend to support live terrain height sculpting, texture splat painting, `.glb` prop placement, walkability grid overrides, and spawn placement.
- **Editor Server Endpoints:** Dev-only endpoints/messages accept updated `sim.bin` and `vis.bin` payloads, write them to `world/areas/...`, and hot-reload active tiles in memory (broadcasting `TileUpdated` to connected dev clients).
- **Compile-Time Release Gating:**
  - Both `server` and `client-wasm` gate all editor logic, UI hooks, and write endpoints behind `#[cfg(feature = "editor")]`.
  - Release builds compile **without** `--features editor` (and use Angular's production build configuration), completely excluding all editor code and endpoints from production binaries.
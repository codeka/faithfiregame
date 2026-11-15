import { effect, inject, Injectable, OnDestroy, signal } from '@angular/core';
import { EngineService } from '../engine.service';
import { EditorEngineHandle, EngineHandle } from '../wasm/client_wasm';

export type AreaKind =
  | 'Outside'
  | { Inside: 'Generated' | 'Ephemeral' | 'Permanent' };

export interface AreaMetadata {
  id: string;
  name: string;
  kind: AreaKind;
  dimensions: [number, number] | null;
}

export interface AssetEntry {
  id: string;
  name: string;
  path: string;
  category: string;
}

export interface SelectedPropInfo {
  id: number;
  assetId: string;
  position: [number, number, number];
  rotationDeg: [number, number, number];
  scale: number;
  castShadow: boolean;
}

export interface EditorStatus {
  areaId: string;
  areaName: string;
  isOutside: boolean;
  currentTile: [number, number];
  dimensions: [number, number] | null;
  cursorPos: [number, number, number] | null;
  dirtyCount: number;
  saveState: string;
  tool: 'heightmap' | 'texture' | 'decoration';
  heightMode: 'raise' | 'lower' | 'smooth' | 'flatten';
  textureLayer: number;
  decorationSubmode: 'place' | 'move' | 'free_move' | 'rotate';
  selectedProp: SelectedPropInfo | null;
  previewRotationDeg: [number, number, number];
  previewScale: number;
  previewCastShadow: boolean;
}

export function formatAreaKind(meta: AreaMetadata): string {
  if (meta.kind === 'Outside') {
    return 'Outside';
  }
  const sub = meta.kind.Inside;
  if (meta.dimensions) {
    return `Inside · ${sub} ${meta.dimensions[0]}×${meta.dimensions[1]}m`;
  }
  return `Inside · ${sub}`;
}

@Injectable({ providedIn: 'root' })
export class EditorEngineService implements OnDestroy {
  private readonly core = inject(EngineService);

  private handle: EditorEngineHandle | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private pendingArea: AreaMetadata | null = null;

  readonly ready = this.core.ready;
  readonly initError = this.core.initError;
  readonly editorEnabled = signal(false);
  readonly status = signal<EditorStatus | null>(null);

  constructor() {
    effect(() => {
      if (!this.core.ready()) {
        return;
      }
      const isEd = EngineHandle.is_editor_enabled();
      this.editorEnabled.set(isEd);
      if (!isEd) {
        return;
      }
      const h = this.ensureHandle();
      if (h && this.pendingArea) {
        h.set_area(JSON.stringify(this.pendingArea));
        this.pendingArea = null;
      }
      this.refreshStatus();
      if (this.pollTimer === null) {
        this.pollTimer = setInterval(() => this.refreshStatus(), 100);
      }
    });
  }

  private ensureHandle(): EditorEngineHandle | null {
    if (this.handle) {
      return this.handle;
    }
    const raw = this.core.rawHandle;
    if (!raw || !EngineHandle.is_editor_enabled()) {
      return null;
    }
    this.editorEnabled.set(true);
    this.handle = new EditorEngineHandle(raw);
    return this.handle;
  }

  private refreshStatus(): void {
    const h = this.ensureHandle();
    if (!h) return;
    try {
      const parsed = JSON.parse(h.get_editor_status_json()) as EditorStatus;
      this.status.set(parsed);
    } catch {
      // ignore transient parse error
    }
  }

  setArea(meta: AreaMetadata): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingArea = meta;
      return;
    }
    h.set_area(JSON.stringify(meta));
    this.refreshStatus();
  }

  setTool(tool: 'heightmap' | 'texture' | 'decoration'): void {
    this.ensureHandle()?.set_editor_tool(tool);
    this.refreshStatus();
  }

  setHeightMode(mode: 'raise' | 'lower' | 'smooth' | 'flatten'): void {
    this.ensureHandle()?.set_height_mode(mode);
    this.refreshStatus();
  }

  setBrushRadius(radius: number): void {
    this.ensureHandle()?.set_brush_radius(radius);
  }

  setBrushStrength(strength: number): void {
    this.ensureHandle()?.set_brush_strength(strength);
  }

  setTextureLayer(layer: number): void {
    this.ensureHandle()?.set_texture_layer(layer);
    this.refreshStatus();
  }

  setDecorationSubmode(submode: 'place' | 'move' | 'free_move' | 'rotate'): void {
    this.ensureHandle()?.set_decoration_submode(submode);
    this.refreshStatus();
  }

  snapSelectedPropToTerrain(): void {
    this.ensureHandle()?.snap_selected_prop_to_terrain();
    this.refreshStatus();
  }

  setDecorationAsset(assetId: string): void {
    this.ensureHandle()?.set_decoration_asset(assetId);
  }

  setPropRotationAxisDeg(axis: 0 | 1 | 2, deg: number): void {
    this.ensureHandle()?.set_selected_prop_rotation_axis_deg(axis, deg);
    this.refreshStatus();
  }

  resetPropRotation(): void {
    this.ensureHandle()?.reset_selected_prop_rotation();
    this.refreshStatus();
  }

  setPropScale(scale: number): void {
    this.ensureHandle()?.set_selected_prop_scale(scale);
    this.refreshStatus();
  }

  setPropCastShadow(castShadow: boolean): void {
    this.ensureHandle()?.set_selected_prop_cast_shadow(castShadow);
    this.refreshStatus();
  }

  duplicateSelectedProp(): void {
    this.ensureHandle()?.duplicate_selected_prop();
    this.refreshStatus();
  }

  deleteSelectedProp(): void {
    this.ensureHandle()?.delete_selected_prop();
    this.refreshStatus();
  }

  saveNow(): void {
    this.ensureHandle()?.save_editor_changes();
    this.refreshStatus();
  }

  ngOnDestroy(): void {
    if (this.pollTimer !== null) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
    this.handle?.free();
    this.handle = null;
  }
}

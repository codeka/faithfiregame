import { Injectable, OnDestroy, signal } from '@angular/core';
import init, { EngineHandle } from './wasm/client_wasm';

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
  rotationDeg: number;
  scale: number;
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
  decorationSubmode: 'place' | 'move' | 'rotate';
  selectedProp: SelectedPropInfo | null;
  previewRotationDeg: number;
  previewScale: number;
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
export class EngineService implements OnDestroy {
  private handle: EngineHandle | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private pendingArea: AreaMetadata | null = null;
  private initializing = false;

  readonly ready = signal(false);
  readonly editorEnabled = signal(false);
  readonly initError = signal<string | null>(null);
  readonly status = signal<EditorStatus | null>(null);

  async init(canvas: HTMLCanvasElement): Promise<void> {
    if (this.handle || this.initializing) {
      return;
    }
    this.initializing = true;
    try {
      await init({ module_or_path: '/wasm/client_wasm_bg.wasm' });
      this.handle = new EngineHandle(canvas);
      const isEd = EngineHandle.is_editor_enabled();
      this.editorEnabled.set(isEd);
      this.ready.set(true);

      if (this.pendingArea) {
        this.handle.set_area(JSON.stringify(this.pendingArea));
        this.pendingArea = null;
      }

      if (isEd) {
        this.refreshStatus();
        this.pollTimer = setInterval(() => this.refreshStatus(), 100);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.initError.set(msg);
      console.error('Failed to initialize Faith & Fire WASM engine:', err);
    } finally {
      this.initializing = false;
    }
  }

  private refreshStatus(): void {
    if (!this.handle) return;
    try {
      const parsed = JSON.parse(this.handle.get_editor_status_json()) as EditorStatus;
      this.status.set(parsed);
    } catch {
      // ignore transient parse error
    }
  }

  setArea(meta: AreaMetadata): void {
    if (!this.handle) {
      this.pendingArea = meta;
      return;
    }
    this.handle.set_area(JSON.stringify(meta));
    this.refreshStatus();
  }

  setTool(tool: 'heightmap' | 'texture' | 'decoration'): void {
    this.handle?.set_editor_tool(tool);
    this.refreshStatus();
  }

  setHeightMode(mode: 'raise' | 'lower' | 'smooth' | 'flatten'): void {
    this.handle?.set_height_mode(mode);
    this.refreshStatus();
  }

  setBrushRadius(radius: number): void {
    this.handle?.set_brush_radius(radius);
  }

  setBrushStrength(strength: number): void {
    this.handle?.set_brush_strength(strength);
  }

  setTextureLayer(layer: number): void {
    this.handle?.set_texture_layer(layer);
    this.refreshStatus();
  }

  setDecorationSubmode(submode: 'place' | 'move' | 'rotate'): void {
    this.handle?.set_decoration_submode(submode);
    this.refreshStatus();
  }

  setDecorationAsset(assetId: string): void {
    this.handle?.set_decoration_asset(assetId);
  }

  setPropRotationDeg(deg: number): void {
    this.handle?.set_selected_prop_rotation_deg(deg);
    this.refreshStatus();
  }

  setPropScale(scale: number): void {
    this.handle?.set_selected_prop_scale(scale);
    this.refreshStatus();
  }

  deleteSelectedProp(): void {
    this.handle?.delete_selected_prop();
    this.refreshStatus();
  }

  saveNow(): void {
    this.handle?.save_editor_changes();
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

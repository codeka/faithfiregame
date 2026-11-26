import { Injectable, OnDestroy, signal } from '@angular/core';
import init, { EngineHandle } from './wasm/client_wasm';

export type GamePhase =
  | 'idle'
  | 'connecting'
  | 'loading_area'
  | 'playing'
  | 'unauthorized'
  | 'error';

export interface GamePopup {
  id: number;
  title: string;
  body: string;
  tone: 'cyan' | 'mint' | 'amber' | 'crimson';
}

export type EquipSlot =
  | 'head'
  | 'body'
  | 'legs'
  | 'feet'
  | 'hands'
  | 'back'
  | 'main_hand'
  | 'off_hand';

export interface LootItemView {
  itemId: string;
  name: string;
  description: string;
  category: string;
  count: number;
  equipped: boolean;
  weight: number;
  previewGlb: string | null;
}

export interface GameLootWindow {
  entityId: number;
  title: string;
  description: string;
  items: LootItemView[];
}

export interface PlayerInventoryItemView {
  itemId: string;
  name: string;
  description: string;
  category: string;
  count: number;
  maxStack: number;
  weight: number;
  equipSlot: EquipSlot | null;
  twoHanded: boolean;
  bonusBagSlots: number;
  equipped: boolean;
  bagSlot: number | null;
  previewGlb: string | null;
}

export interface GamePlayerInventory {
  items: PlayerInventoryItemView[];
  bagCapacity: number;
  totalWeight: number;
  maxWeight: number;
}

export interface GameStatus {
  phase: GamePhase;
  playerId: number | null;
  areaId: string;
  areaName: string;
  playerPos: [number, number, number] | null;
  freshCryoWake: boolean;
  wakeReady: boolean;
  popup?: GamePopup | null;
  loot?: GameLootWindow | null;
  inventory?: GamePlayerInventory | null;
  error: string | null;
}

type PendingEngineAction = { kind: 'landing' } | { kind: 'connect'; token: string };

@Injectable({ providedIn: 'root' })
export class EngineService implements OnDestroy {
  private handle: EngineHandle | null = null;
  private initializing = false;
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private pendingAction: PendingEngineAction | null = null;
  private thumbCanvas: HTMLCanvasElement | null = null;

  readonly ready = signal(false);
  readonly initError = signal<string | null>(null);
  readonly editorBuild = signal(false);
  readonly gameStatus = signal<GameStatus | null>(null);
  readonly itemThumbnails = signal<Record<string, string>>({});

  get rawHandle(): EngineHandle | null {
    return this.handle;
  }

  async init(canvas: HTMLCanvasElement): Promise<void> {
    if (this.handle || this.initializing) {
      return;
    }
    this.initializing = true;
    try {
      await init({ module_or_path: fetch('/wasm/client_wasm_bg.wasm', { cache: 'no-cache' }) });
      this.editorBuild.set(EngineHandle.is_editor_enabled());
      this.handle = new EngineHandle(canvas);

      const action = this.pendingAction;
      this.pendingAction = null;
      if (action?.kind === 'connect') {
        this.handle.connect_game_session(action.token);
        this.refreshGameStatus();
        this.startPolling();
      } else if (action?.kind === 'landing') {
        this.handle.enter_landing_mode();
      }

      this.ready.set(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.initError.set(msg);
      console.error('Failed to initialize Faith & Fire WASM engine:', err);
    } finally {
      this.initializing = false;
    }
  }

  enterLandingMode(): void {
    this.stopGamePolling();
    this.gameStatus.set(null);
    if (!this.handle) {
      this.pendingAction = { kind: 'landing' };
      return;
    }
    this.handle.enter_landing_mode();
  }

  connectGameSession(token: string): void {
    if (!this.handle) {
      this.pendingAction = { kind: 'connect', token };
      return;
    }
    this.handle.connect_game_session(token);
    this.refreshGameStatus();
    this.startPolling();
  }

  disconnectGameSession(): void {
    this.enterLandingMode();
  }

  dismissPopup(): void {
    this.handle?.dismiss_game_popup();
    this.refreshGameStatus();
  }

  takeLootItem(entityId: number, itemId: string): void {
    this.handle?.loot_take_item(entityId, itemId);
    this.refreshGameStatus();
  }

  takeAllLoot(entityId: number): void {
    this.handle?.loot_take_all(entityId);
    this.refreshGameStatus();
  }

  dismissLoot(): void {
    this.handle?.dismiss_loot_window();
    this.refreshGameStatus();
  }

  equipInventoryItem(bagSlot: number, targetSlot?: EquipSlot | null): void {
    this.handle?.equip_inventory_item(bagSlot, targetSlot ?? '');
    this.refreshGameStatus();
  }

  unequipInventorySlot(slot: EquipSlot, targetBagSlot?: number | null): void {
    this.handle?.unequip_inventory_slot(slot, targetBagSlot ?? -1);
    this.refreshGameStatus();
  }

  moveInventoryItem(fromSlot: number, toSlot: number): void {
    this.handle?.move_inventory_item(fromSlot, toSlot);
    this.refreshGameStatus();
  }

  splitInventoryStack(fromSlot: number, toSlot: number, count: number): void {
    this.handle?.split_inventory_stack(fromSlot, toSlot, count);
    this.refreshGameStatus();
  }

  renderCharacterPreview(width: number, height: number, yawRad: number): Uint8Array | null {
    return this.handle?.render_character_preview_rgba(width, height, yawRad) ?? null;
  }

  stopGamePolling(): void {
    if (this.pollTimer !== null) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
  }

  private startPolling(): void {
    if (this.pollTimer !== null) {
      return;
    }
    this.pollTimer = setInterval(() => this.refreshGameStatus(), 80);
  }

  private refreshGameStatus(): void {
    if (!this.handle) {
      return;
    }
    try {
      const parsed = JSON.parse(this.handle.get_game_status_json()) as GameStatus;
      this.gameStatus.set(parsed);
      this.ensureItemThumbnails(parsed);
    } catch {
      // Ignore transient parse errors.
    }
  }

  private ensureItemThumbnails(status: GameStatus): void {
    if (!this.handle) {
      return;
    }
    const paths = new Set<string>();
    for (const item of status.inventory?.items ?? []) {
      if (item.previewGlb) paths.add(item.previewGlb);
    }
    for (const item of status.loot?.items ?? []) {
      if (item.previewGlb) paths.add(item.previewGlb);
    }
    if (paths.size === 0) {
      return;
    }
    const current = this.itemThumbnails();
    let added: Record<string, string> | null = null;
    for (const path of paths) {
      if (current[path]) {
        continue;
      }
      const rgba = this.handle.render_item_thumbnail_rgba(path, 96);
      if (!rgba || rgba.length !== 96 * 96 * 4) {
        continue;
      }
      if (!this.thumbCanvas) {
        this.thumbCanvas = document.createElement('canvas');
        this.thumbCanvas.width = 96;
        this.thumbCanvas.height = 96;
      }
      const ctx = this.thumbCanvas.getContext('2d');
      if (!ctx) {
        continue;
      }
      const imgData = new ImageData(new Uint8ClampedArray(rgba), 96, 96);
      ctx.putImageData(imgData, 0, 0);
      added ??= {};
      added[path] = this.thumbCanvas.toDataURL('image/png');
    }
    if (added) {
      this.itemThumbnails.update((prev) => ({ ...prev, ...added }));
    }
  }

  resize(width: number, height: number): void {
    this.handle?.resize(width, height);
  }

  ngOnDestroy(): void {
    this.stopGamePolling();
    this.handle?.free();
    this.handle = null;
  }
}

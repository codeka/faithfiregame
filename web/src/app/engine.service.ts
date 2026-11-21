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

export interface LootItemView {
  itemId: string;
  name: string;
  description: string;
  category: string;
  count: number;
  equipped: boolean;
}

export interface GameLootWindow {
  entityId: number;
  title: string;
  description: string;
  items: LootItemView[];
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
  error: string | null;
}

type PendingEngineAction = { kind: 'landing' } | { kind: 'connect'; token: string };

@Injectable({ providedIn: 'root' })
export class EngineService implements OnDestroy {
  private handle: EngineHandle | null = null;
  private initializing = false;
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private pendingAction: PendingEngineAction | null = null;

  readonly ready = signal(false);
  readonly initError = signal<string | null>(null);
  readonly editorBuild = signal(false);
  readonly gameStatus = signal<GameStatus | null>(null);

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
    } catch {
      // Ignore transient parse errors.
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

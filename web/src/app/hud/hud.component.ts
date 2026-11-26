import { Component, computed, effect, HostListener, inject, OnDestroy, signal } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { AuthService } from '../auth/auth.service';
import { EngineService, GamePlayerInventory } from '../engine.service';
import { WakeOverlayState } from './hud.types';
import { HudButtonComponent } from './primitives/hud-button/hud-button.component';
import { HudFrameComponent } from './primitives/hud-frame/hud-frame.component';
import { AreaTelemetryComponent } from './widgets/area-telemetry/area-telemetry.component';
import { InventoryPanelComponent } from './widgets/inventory-panel/inventory-panel.component';
import { SystemBarComponent } from './widgets/system-bar/system-bar.component';
import { WakeOverlayComponent } from './widgets/wake-overlay/wake-overlay.component';

export type { WakeOverlayState } from './hud.types';

const EMPTY_INVENTORY: GamePlayerInventory = {
  items: [],
  bagCapacity: 20,
  totalWeight: 0,
  maxWeight: 30,
};

function isEditableEventTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) {
    return false;
  }
  if (
    target.getAttribute('contenteditable') !== null &&
    target.getAttribute('contenteditable')?.toLowerCase() !== 'false'
  ) {
    return true;
  }
  const tag = target.tagName;
  if (tag === 'TEXTAREA' || tag === 'SELECT') {
    return true;
  }
  if (tag === 'INPUT') {
    const type = target.getAttribute('type')?.toLowerCase() ?? '';
    return type !== 'range' && type !== 'checkbox' && type !== 'radio' && type !== 'button';
  }
  return false;
}

@Component({
  selector: 'app-hud',
  standalone: true,
  imports: [
    RouterLink,
    HudButtonComponent,
    HudFrameComponent,
    AreaTelemetryComponent,
    InventoryPanelComponent,
    SystemBarComponent,
    WakeOverlayComponent,
  ],
  templateUrl: './hud.component.html',
  styleUrl: './hud.component.css',
})
export class HudComponent implements OnDestroy {
  readonly auth = inject(AuthService);
  readonly engine = inject(EngineService);
  private readonly router = inject(Router);

  readonly startingPlay = signal(false);
  readonly playError = signal<string | null>(null);
  readonly wakeOverlayState = signal<WakeOverlayState>('done');
  readonly inventoryOpen = signal(false);

  readonly activeInventory = computed<GamePlayerInventory>(
    () => this.engine.gameStatus()?.inventory ?? EMPTY_INVENTORY,
  );

  private wakeTriggered = false;
  private wakeFallbackTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    effect(() => {
      const token = this.auth.sessionToken();
      this.clearWakeTimer();
      this.wakeTriggered = false;
      if (token) {
        this.playError.set(null);
        this.wakeOverlayState.set('closed');
        this.engine.connectGameSession(token);
      } else {
        this.inventoryOpen.set(false);
        this.wakeOverlayState.set('done');
        this.engine.enterLandingMode();
      }
    });

    effect(() => {
      if (!this.auth.hasSession()) {
        return;
      }
      const st = this.engine.gameStatus();
      if (!st) {
        return;
      }
      if (st.phase === 'unauthorized') {
        void this.auth.logout(false);
        return;
      }
      if (st.wakeReady && !this.wakeTriggered) {
        this.wakeTriggered = true;
        const mode: WakeOverlayState = st.freshCryoWake ? 'animating' : 'fading';
        this.wakeOverlayState.set(mode);
        this.scheduleWakeComplete(mode === 'animating' ? 3700 : 380);
      }
    });
  }

  @HostListener('window:keydown', ['$event'])
  onWindowKeyDown(ev: KeyboardEvent): void {
    if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.repeat || isEditableEventTarget(ev.target)) {
      return;
    }
    if (!this.auth.hasSession() || this.engine.gameStatus()?.phase !== 'playing') {
      return;
    }
    if (ev.key === 'i' || ev.key === 'I') {
      ev.preventDefault();
      this.toggleInventory();
    } else if (ev.key === 'Escape' && this.inventoryOpen()) {
      ev.preventDefault();
      this.inventoryOpen.set(false);
    }
  }

  toggleInventory(): void {
    this.inventoryOpen.update((open) => !open);
  }

  closeInventory(): void {
    this.inventoryOpen.set(false);
  }

  async onPlay(): Promise<void> {
    if (this.startingPlay()) {
      return;
    }
    this.playError.set(null);
    this.startingPlay.set(true);
    try {
      await this.auth.startFreshPlay();
    } catch (err) {
      this.playError.set(err instanceof Error ? err.message : String(err));
    } finally {
      this.startingPlay.set(false);
    }
  }

  onGoLogin(): void {
    void this.router.navigate(['/login']);
  }

  async onLeave(): Promise<void> {
    this.clearWakeTimer();
    this.inventoryOpen.set(false);
    this.engine.disconnectGameSession();
    await this.auth.logout();
  }

  finishWakeOverlay(): void {
    this.clearWakeTimer();
    this.wakeOverlayState.set('done');
  }

  private scheduleWakeComplete(delayMs: number): void {
    this.clearWakeTimer();
    this.wakeFallbackTimer = setTimeout(() => this.finishWakeOverlay(), delayMs);
  }

  private clearWakeTimer(): void {
    if (this.wakeFallbackTimer !== null) {
      clearTimeout(this.wakeFallbackTimer);
      this.wakeFallbackTimer = null;
    }
  }

  ngOnDestroy(): void {
    this.clearWakeTimer();
    this.engine.stopGamePolling();
  }
}

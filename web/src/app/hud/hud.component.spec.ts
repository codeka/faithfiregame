import { signal, WritableSignal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { AuthService, SESSION_COOKIE } from '../auth/auth.service';
import {
  EngineService,
  EquipSlot,
  GamePlayerInventory,
  GameStatus,
} from '../engine.service';
import { LoginComponent } from '../login/login.component';
import { HudComponent } from './hud.component';

function clearSessionCookie(): void {
  document.cookie = `${SESSION_COOKIE}=; Path=/; Max-Age=0`;
}

describe('HudComponent & LoginComponent entry flow', () => {
  let gameStatusSig: WritableSignal<GameStatus | null>;
  let editorBuildSig: WritableSignal<boolean>;
  let itemThumbnailsSig: WritableSignal<Record<string, string>>;
  let connectCalls: string[];
  let landingCalls: number;
  let stopPollingCalls: number;
  let dismissPopupCalls: number;
  let lootTakeCalls: { entityId: number; itemId: string }[];
  let lootTakeAllCalls: number[];
  let dismissLootCalls: number;
  let equipCalls: { bagSlot: number; targetSlot: EquipSlot | null }[];
  let unequipCalls: { slot: EquipSlot; targetBagSlot: number | null }[];
  let moveCalls: { fromSlot: number; toSlot: number }[];
  let splitCalls: { fromSlot: number; toSlot: number; count: number }[];
  let origFetch: typeof globalThis.fetch;

  function configureTestBed(): void {
    const mockEngine = {
      ready: signal(true),
      initError: signal<string | null>(null),
      editorBuild: editorBuildSig,
      gameStatus: gameStatusSig,
      itemThumbnails: itemThumbnailsSig,
      enterLandingMode: () => {
        landingCalls++;
        gameStatusSig.set(null);
      },
      connectGameSession: (token: string) => {
        connectCalls.push(token);
      },
      disconnectGameSession: () => {
        landingCalls++;
        gameStatusSig.set(null);
      },
      dismissPopup: () => {
        dismissPopupCalls++;
        gameStatusSig.update((s) => (s ? { ...s, popup: null } : s));
      },
      takeLootItem: (entityId: number, itemId: string) => {
        lootTakeCalls.push({ entityId, itemId });
      },
      takeAllLoot: (entityId: number) => {
        lootTakeAllCalls.push(entityId);
      },
      dismissLoot: () => {
        dismissLootCalls++;
        gameStatusSig.update((s) => (s ? { ...s, loot: null } : s));
      },
      equipInventoryItem: (bagSlot: number, targetSlot: EquipSlot | null = null) => {
        equipCalls.push({ bagSlot, targetSlot });
      },
      unequipInventorySlot: (slot: EquipSlot, targetBagSlot: number | null = null) => {
        unequipCalls.push({ slot, targetBagSlot });
      },
      moveInventoryItem: (fromSlot: number, toSlot: number) => {
        moveCalls.push({ fromSlot, toSlot });
      },
      splitInventoryStack: (fromSlot: number, toSlot: number, count: number) => {
        splitCalls.push({ fromSlot, toSlot, count });
      },
      renderCharacterPreview: () => null,
      stopGamePolling: () => {
        stopPollingCalls++;
      },
    };

    TestBed.configureTestingModule({
      imports: [HudComponent, LoginComponent],
      providers: [
        AuthService,
        { provide: EngineService, useValue: mockEngine },
        provideRouter([
          { path: '', component: HudComponent },
          { path: 'login', component: LoginComponent },
        ]),
      ],
    });
  }

  beforeEach(() => {
    clearSessionCookie();
    origFetch = globalThis.fetch;
    gameStatusSig = signal<GameStatus | null>(null);
    editorBuildSig = signal(false);
    itemThumbnailsSig = signal<Record<string, string>>({});
    connectCalls = [];
    landingCalls = 0;
    stopPollingCalls = 0;
    dismissPopupCalls = 0;
    lootTakeCalls = [];
    lootTakeAllCalls = [];
    dismissLootCalls = 0;
    equipCalls = [];
    unequipCalls = [];
    moveCalls = [];
    splitCalls = [];
  });

  afterEach(() => {
    globalThis.fetch = origFetch;
    clearSessionCookie();
  });

  it('shows Play and Login buttons when no ff_session cookie exists', async () => {
    configureTestBed();
    const fixture: ComponentFixture<HudComponent> = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    const text = (fixture.nativeElement as HTMLElement).textContent ?? '';
    const buttons = Array.from(
      (fixture.nativeElement as HTMLElement).querySelectorAll('button'),
    ).map((b) => b.textContent?.trim());
    expect(text).toContain('Faith & Fire');
    const landingLogo = (fixture.nativeElement as HTMLElement).querySelector(
      'img.landing-logo',
    ) as HTMLImageElement | null;
    expect(landingLogo?.getAttribute('src')).toBe('/logo-landing.png');
    expect(buttons).toContain('Play');
    expect(buttons).toContain('Login');
    expect(landingCalls).toBeGreaterThan(0);
    expect(connectCalls).toEqual([]);
  });

  it('clicking Play calls POST /api/auth/play, stores ff_session cookie, and connects', async () => {
    const fetchCalls: { url: string; method: string }[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      fetchCalls.push({ url: String(input), method: init?.method ?? 'GET' });
      return new Response(
        JSON.stringify({ player_id: 42, session_token: 'guest-tok-42' }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    }) as typeof fetch;

    configureTestBed();
    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    const playBtn = Array.from(
      (fixture.nativeElement as HTMLElement).querySelectorAll('button'),
    ).find((b) => b.textContent?.trim() === 'Play');
    expect(playBtn).toBeTruthy();
    playBtn!.click();
    await new Promise((r) => setTimeout(r, 0));
    fixture.detectChanges();
    await fixture.whenStable();

    expect(fetchCalls).toEqual([{ url: '/api/auth/play', method: 'POST' }]);
    expect(document.cookie).toContain(`${SESSION_COOKIE}=guest-tok-42`);
    expect(connectCalls).toEqual(['guest-tok-42']);
    expect(
      (fixture.nativeElement as HTMLElement).querySelector('.landing-card'),
    ).toBeNull();
  });

  it('skips landing and plays cryo wake animation when ff_session cookie already exists', async () => {
    document.cookie = `${SESSION_COOKIE}=existing-tok-9; Path=/`;
    configureTestBed();

    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    expect(connectCalls).toEqual(['existing-tok-9']);
    expect(
      (fixture.nativeElement as HTMLElement).querySelector('.landing-card'),
    ).toBeNull();
    expect(fixture.componentInstance.wakeOverlayState()).toBe('closed');

    gameStatusSig.set({
      phase: 'playing',
      playerId: 9,
      areaId: 'cryo_prison',
      areaName: 'Cryo Prison',
      playerPos: [7.4, 0, 7.4],
      freshCryoWake: true,
      wakeReady: true,
      error: null,
    });
    fixture.detectChanges();
    await fixture.whenStable();

    expect(fixture.componentInstance.wakeOverlayState()).toBe('animating');
    const overlay = (fixture.nativeElement as HTMLElement).querySelector('.wake-overlay');
    expect(overlay?.classList.contains('wake-animating')).toBe(true);
    expect(
      (fixture.nativeElement as HTMLElement).querySelector('.area-name')?.textContent?.trim(),
    ).toBe('Cryo Prison');

    fixture.destroy();
    expect(stopPollingCalls).toBe(1);
  });

  it('clears ff_session cookie and returns to Play/Login landing on unauthorized status', async () => {
    document.cookie = `${SESSION_COOKIE}=stale-tok; Path=/`;
    configureTestBed();

    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    gameStatusSig.set({
      phase: 'unauthorized',
      playerId: null,
      areaId: '',
      areaName: '',
      playerPos: null,
      freshCryoWake: false,
      wakeReady: false,
      error: null,
    });
    fixture.detectChanges();
    await new Promise((r) => setTimeout(r, 0));
    fixture.detectChanges();
    await fixture.whenStable();

    expect(document.cookie).not.toContain('stale-tok');
    expect(
      (fixture.nativeElement as HTMLElement).querySelector('.landing-card'),
    ).not.toBeNull();
  });

  it('LoginComponent submits credentials to POST /api/auth/login and redirects to /', async () => {
    const fetchCalls: { url: string; body: string }[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      fetchCalls.push({
        url: String(input),
        body: typeof init?.body === 'string' ? init.body : '',
      });
      return new Response(
        JSON.stringify({ player_id: 7, session_token: 'login-tok-7' }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      );
    }) as typeof fetch;

    configureTestBed();
    const router = TestBed.inject(Router);
    let navigatedTo: string | null = null;
    router.navigate = (async (commands: unknown[]) => {
      navigatedTo = commands.join('/');
      return true;
    }) as typeof router.navigate;

    const fixture = TestBed.createComponent(LoginComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    const loginLogo = (fixture.nativeElement as HTMLElement).querySelector(
      'img.login-logo',
    ) as HTMLImageElement | null;
    expect(loginLogo?.getAttribute('src')).toBe('/logo-icon.png');

    fixture.componentInstance.username = 'survivor';
    fixture.componentInstance.password = 'secret-pass';
    await fixture.componentInstance.onSubmit();
    fixture.detectChanges();

    expect(fetchCalls).toEqual([
      {
        url: '/api/auth/login',
        body: JSON.stringify({ username: 'survivor', password: 'secret-pass' }),
      },
    ]);
    expect(document.cookie).toContain(`${SESSION_COOKIE}=login-tok-7`);
    expect(navigatedTo).toBe('/');
  });

  it('renders top-left area telemetry with formatted coordinates and controls hint while playing', async () => {
    document.cookie = `${SESSION_COOKIE}=play-tok-1; Path=/`;
    configureTestBed();

    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    gameStatusSig.set({
      phase: 'playing',
      playerId: 1,
      areaId: 'cryo_prison',
      areaName: 'Cryo Prison',
      playerPos: [7.4, 0, 7.4],
      freshCryoWake: false,
      wakeReady: true,
      error: null,
    });
    fixture.detectChanges();
    await fixture.whenStable();

    const host = fixture.nativeElement as HTMLElement;
    const telemetry = host.querySelector('app-hud-area-telemetry');
    expect(telemetry).not.toBeNull();
    expect(telemetry?.getAttribute('data-area-id')).toBe('cryo_prison');
    expect(telemetry?.querySelector('.area-name')?.textContent?.trim()).toBe('Cryo Prison');
    expect(telemetry?.querySelector('app-hud-badge')?.textContent?.trim()).toBe('07 · 07');
    expect(telemetry?.querySelector('.controls-hint')?.textContent?.trim()).toContain(
      'Click floor to walk',
    );

    gameStatusSig.set({
      phase: 'playing',
      playerId: 1,
      areaId: 'cryo_prison',
      areaName: '',
      playerPos: null,
      freshCryoWake: false,
      wakeReady: true,
      error: null,
    });
    fixture.detectChanges();
    await fixture.whenStable();

    expect(telemetry?.querySelector('.area-name')?.textContent?.trim()).toBe('Cryo Prison');
    expect(telemetry?.querySelector('app-hud-badge')).toBeNull();
  });

  it('toggles Editor link via editorBuild signal and disconnects session on Leave click', async () => {
    document.cookie = `${SESSION_COOKIE}=play-tok-2; Path=/`;
    const fetchCalls: { url: string; method: string }[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      fetchCalls.push({ url: String(input), method: init?.method ?? 'GET' });
      return new Response(null, { status: 200 });
    }) as typeof fetch;

    configureTestBed();
    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    gameStatusSig.set({
      phase: 'playing',
      playerId: 2,
      areaId: 'cryo_prison',
      areaName: 'Cryo Prison',
      playerPos: [0, 0, 0],
      freshCryoWake: false,
      wakeReady: true,
      error: null,
    });
    fixture.detectChanges();
    await fixture.whenStable();

    const host = fixture.nativeElement as HTMLElement;
    const systemBar = host.querySelector('app-hud-system-bar');
    expect(systemBar).not.toBeNull();
    expect(systemBar?.querySelector('a[routerLink="/editor"]')).toBeNull();

    editorBuildSig.set(true);
    fixture.detectChanges();
    await fixture.whenStable();

    const editorLink = systemBar?.querySelector('a[href="/editor"]');
    expect(editorLink?.textContent?.trim()).toBe('Editor');

    const leaveBtn = Array.from(systemBar?.querySelectorAll('button') ?? []).find(
      (b) => b.textContent?.trim() === 'Leave',
    );
    expect(leaveBtn).toBeTruthy();
    const landingBefore = landingCalls;
    leaveBtn!.click();
    await new Promise((r) => setTimeout(r, 0));
    fixture.detectChanges();
    await fixture.whenStable();

    expect(landingCalls).toBeGreaterThan(landingBefore);
    expect(fetchCalls).toEqual([{ url: '/api/auth/logout', method: 'POST' }]);
    expect(document.cookie).not.toContain('play-tok-2');
    expect(host.querySelector('.landing-card')).not.toBeNull();
  });

  it('displays server popup modal during gameplay and dismisses on Acknowledge click', async () => {
    document.cookie = `${SESSION_COOKIE}=play-tok-3; Path=/`;
    configureTestBed();
    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    gameStatusSig.set({
      phase: 'playing',
      playerId: 3,
      areaId: 'cryo_prison',
      areaName: 'Cryo Prison',
      playerPos: [4, 0, 4],
      freshCryoWake: false,
      wakeReady: true,
      popup: {
        id: 1,
        title: 'Airlock Override',
        body: 'Decontamination cycle complete.',
        tone: 'mint',
      },
      error: null,
    });
    fixture.detectChanges();
    await fixture.whenStable();

    const host = fixture.nativeElement as HTMLElement;
    expect(host.querySelector('.hud-popup-title')?.textContent?.trim()).toBe('Airlock Override');
    expect(host.querySelector('.hud-popup-body')?.textContent?.trim()).toBe(
      'Decontamination cycle complete.',
    );

    const ackBtn = Array.from(host.querySelectorAll('.hud-popup-actions button')).find(
      (b) => b.textContent?.trim() === 'Acknowledge',
    ) as HTMLButtonElement | undefined;
    expect(ackBtn).toBeDefined();
    ackBtn!.click();
    fixture.detectChanges();
    await fixture.whenStable();

    expect(dismissPopupCalls).toBe(1);
    expect(host.querySelector('.hud-popup-backdrop')).toBeNull();
  });

  it('renders loot dialog items, dispatches Take / Take All / Close, and shows empty message when looted', async () => {
    document.cookie = `${SESSION_COOKIE}=play-tok-loot; Path=/`;
    configureTestBed();
    const fixture = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    await fixture.whenStable();

    gameStatusSig.set({
      phase: 'playing',
      playerId: 4,
      areaId: 'cryo_prison',
      areaName: 'Cryo Prison',
      playerPos: [18, 0, 12],
      freshCryoWake: false,
      wakeReady: true,
      loot: {
        entityId: 1001,
        title: 'Fallen Security Guard',
        description: 'Still clutching his carbine.',
        items: [
          {
            itemId: 'guard_rifle',
            name: 'Guard Carbine',
            description: 'Security carbine.',
            category: 'Weapon',
            count: 1,
            equipped: true,
            weight: 3.6,
            previewGlb: '/world/assets/characters/weapon_guard_rifle.glb',
          },
        ],
      },
      inventory: null,
      error: null,
    });
    fixture.detectChanges();
    await fixture.whenStable();

    const host = fixture.nativeElement as HTMLElement;
    expect(host.querySelector('.hud-loot-frame')).not.toBeNull();
    expect(host.querySelector('#hud-loot-title')?.textContent?.trim()).toBe(
      'Fallen Security Guard',
    );
    expect(host.querySelector('.hud-loot-item-name')?.textContent?.trim()).toBe('Guard Carbine');
    expect(host.querySelector('.hud-loot-tag.equipped')?.textContent?.trim()).toBe('Equipped');

    const takeBtn = host.querySelector('.loot-take-btn') as HTMLButtonElement | null;
    const takeAllBtn = host.querySelector('.loot-take-all-btn') as HTMLButtonElement | null;
    expect(takeBtn).not.toBeNull();
    expect(takeAllBtn).not.toBeNull();

    takeBtn!.click();
    takeAllBtn!.click();
    expect(lootTakeCalls).toEqual([{ entityId: 1001, itemId: 'guard_rifle' }]);
    expect(lootTakeAllCalls).toEqual([1001]);

    gameStatusSig.update((s) =>
      s
        ? {
            ...s,
            loot: {
              entityId: 1001,
              title: 'Fallen Security Guard',
              description: 'Still clutching his carbine.',
              items: [],
            },
          }
        : s,
    );
    fixture.detectChanges();
    await fixture.whenStable();

    expect(host.querySelector('.loot-take-all-btn')).toBeNull();
    expect(host.querySelector('.hud-loot-empty')?.textContent?.trim()).toBe(
      'Nothing left to take.',
    );

    const closeBtn = host.querySelector('.loot-close-btn') as HTMLButtonElement | null;
    expect(closeBtn).not.toBeNull();
    closeBtn!.click();
    fixture.detectChanges();
    await fixture.whenStable();

    expect(dismissLootCalls).toBe(1);
    expect(host.querySelector('.hud-loot-frame')).toBeNull();
  });

  function sampleInventory(): GamePlayerInventory {
    return {
      bagCapacity: 25,
      totalWeight: 5.8,
      maxWeight: 30,
      items: [
        {
          itemId: 'salvage_rig_pack',
          name: 'Salvage Rig Pack',
          description: 'Adds 5 extra backpack slots.',
          category: 'Back',
          count: 1,
          maxStack: 1,
          weight: 1.5,
          equipSlot: 'back',
          twoHanded: false,
          bonusBagSlots: 5,
          equipped: true,
          bagSlot: null,
          previewGlb: '/world/assets/characters/back_rig.glb',
        },
        {
          itemId: 'guard_rifle',
          name: 'Guard Carbine',
          description: 'Two-handed security carbine.',
          category: 'Weapon',
          count: 1,
          maxStack: 1,
          weight: 3.6,
          equipSlot: 'main_hand',
          twoHanded: true,
          bonusBagSlots: 0,
          equipped: true,
          bagSlot: null,
          previewGlb: '/world/assets/characters/weapon_guard_rifle.glb',
        },
        {
          itemId: 'ration_pack',
          name: 'Ration Pack',
          description: 'Nutrient paste bricks.',
          category: 'Consumable',
          count: 4,
          maxStack: 10,
          weight: 0.175,
          equipSlot: null,
          twoHanded: false,
          bonusBagSlots: 0,
          equipped: false,
          bagSlot: 0,
          previewGlb: '/world/assets/items/ration_pack.glb',
        },
        {
          itemId: 'security_helmet',
          name: 'Security Helmet',
          description: 'Visor helm.',
          category: 'Head',
          count: 1,
          maxStack: 1,
          weight: 0.8,
          equipSlot: 'head',
          twoHanded: false,
          bonusBagSlots: 0,
          equipped: false,
          bagSlot: 2,
          previewGlb: '/world/assets/characters/helmet_guard.glb',
        },
      ],
    };
  }

  it('toggles inventory via button and hotkey I, closes on Escape, and ignores editable inputs', () => {
    document.cookie = `${SESSION_COOKIE}=inv-tok-1; Path=/`;
    configureTestBed();
    gameStatusSig.set({
      phase: 'playing',
      playerId: 42,
      areaId: 'cryo_prison',
      areaName: 'Cryo Holding Block',
      playerPos: [1, 0, 1],
      freshCryoWake: false,
      wakeReady: true,
      inventory: sampleInventory(),
      error: null,
    });

    const fixture: ComponentFixture<HudComponent> = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    const host = fixture.nativeElement as HTMLElement;

    expect(host.querySelector('app-hud-inventory-panel')).toBeNull();

    const invBtn = host.querySelector<HTMLButtonElement>('.system-inv-btn');
    expect(invBtn).not.toBeNull();
    invBtn!.click();
    fixture.detectChanges();
    expect(host.querySelector('app-hud-inventory-panel')).not.toBeNull();

    // Close via close button inside panel.
    host.querySelector<HTMLButtonElement>('.inv-close-btn')!.click();
    fixture.detectChanges();
    expect(host.querySelector('app-hud-inventory-panel')).toBeNull();

    // Hotkey 'i' opens, Escape closes.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'i', bubbles: true }));
    fixture.detectChanges();
    expect(host.querySelector('app-hud-inventory-panel')).not.toBeNull();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    fixture.detectChanges();
    expect(host.querySelector('app-hud-inventory-panel')).toBeNull();

    // Typing 'i' inside an editable input must NOT toggle the inventory panel.
    const input = document.createElement('input');
    host.appendChild(input);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'i', bubbles: true }));
    fixture.detectChanges();
    expect(host.querySelector('app-hud-inventory-panel')).toBeNull();

    fixture.destroy();
  });

  it('renders 8 equip slots, 2H dimmed off-hand ghost, preview canvas, thumbnails, and weight/slot counters', () => {
    document.cookie = `${SESSION_COOKIE}=inv-tok-2; Path=/`;
    configureTestBed();
    itemThumbnailsSig.set({
      '/world/assets/characters/weapon_guard_rifle.glb': 'data:image/png;base64,RIFLE',
      '/world/assets/items/ration_pack.glb': 'data:image/png;base64,RATION',
    });
    gameStatusSig.set({
      phase: 'playing',
      playerId: 42,
      areaId: 'cryo_prison',
      areaName: 'Cryo Holding Block',
      playerPos: [1, 0, 1],
      freshCryoWake: false,
      wakeReady: true,
      inventory: sampleInventory(),
      error: null,
    });

    const fixture: ComponentFixture<HudComponent> = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    hostOpenInventory(fixture);
    const host = fixture.nativeElement as HTMLElement;

    const slots: EquipSlot[] = [
      'head',
      'body',
      'legs',
      'feet',
      'hands',
      'back',
      'main_hand',
      'off_hand',
    ];
    for (const slot of slots) {
      expect(host.querySelector(`.equip-cell[data-slot="${slot}"]`)).not.toBeNull();
    }

    expect(host.querySelector('canvas.preview-canvas')).not.toBeNull();
    expect(host.querySelectorAll('.bag-cell').length).toBe(25);

    const offHand = host.querySelector('.equip-cell[data-slot="off_hand"]')!;
    expect(offHand.classList.contains('is-two-handed-ghost')).toBe(true);
    expect(offHand.querySelector('.slot-badge')?.textContent?.trim()).toBe('2H');

    const bagRationImg = host.querySelector<HTMLImageElement>(
      '.bag-cell[data-bag-slot="0"] img.item-thumb',
    );
    expect(bagRationImg?.getAttribute('src')).toBe('data:image/png;base64,RATION');
    expect(
      host.querySelector('.bag-cell[data-bag-slot="0"] .count-badge')?.textContent?.trim(),
    ).toBe('×4');

    const statText = Array.from(host.querySelectorAll('.inv-stat-pill'))
      .map((el) => el.textContent?.trim())
      .join(' | ');
    expect(statText).toContain('2/25');
    expect(statText).toContain('5.8/30 kg');

    fixture.destroy();
  });

  it('supports double-click, drag-and-drop reordering/equipping/unequipping, and right-click stack splitting', () => {
    document.cookie = `${SESSION_COOKIE}=inv-tok-3; Path=/`;
    configureTestBed();
    gameStatusSig.set({
      phase: 'playing',
      playerId: 42,
      areaId: 'cryo_prison',
      areaName: 'Cryo Holding Block',
      playerPos: [1, 0, 1],
      freshCryoWake: false,
      wakeReady: true,
      inventory: sampleInventory(),
      error: null,
    });

    const fixture: ComponentFixture<HudComponent> = TestBed.createComponent(HudComponent);
    fixture.detectChanges();
    hostOpenInventory(fixture);
    const host = fixture.nativeElement as HTMLElement;

    // Double-click equips bag slot 2 (Security Helmet) and unequips back slot.
    host.querySelector('.bag-cell[data-bag-slot="2"]')!.dispatchEvent(new MouseEvent('dblclick'));
    expect(equipCalls).toEqual([{ bagSlot: 2, targetSlot: null }]);

    host.querySelector('.equip-cell[data-slot="back"]')!.dispatchEvent(new MouseEvent('dblclick'));
    expect(unequipCalls).toEqual([{ slot: 'back', targetBagSlot: null }]);

    // Drag bag slot 0 (Ration Pack) -> bag slot 5 -> moveInventoryItem(0, 5).
    const bag0 = host.querySelector('.bag-cell[data-bag-slot="0"]')!;
    const bag5 = host.querySelector('.bag-cell[data-bag-slot="5"]')!;
    bag0.dispatchEvent(new Event('dragstart', { bubbles: true }));
    bag5.dispatchEvent(new Event('drop', { bubbles: true }));
    expect(moveCalls).toEqual([{ fromSlot: 0, toSlot: 5 }]);

    // Drag bag slot 2 (helmet) -> equip head -> equipInventoryItem(2, 'head').
    const bag2 = host.querySelector('.bag-cell[data-bag-slot="2"]')!;
    const headCell = host.querySelector('.equip-cell[data-slot="head"]')!;
    bag2.dispatchEvent(new Event('dragstart', { bubbles: true }));
    headCell.dispatchEvent(new Event('drop', { bubbles: true }));
    expect(equipCalls[1]).toEqual({ bagSlot: 2, targetSlot: 'head' });

    // Drag equip main_hand -> bag slot 6 -> unequipInventorySlot('main_hand', 6).
    const mainHand = host.querySelector('.equip-cell[data-slot="main_hand"]')!;
    const bag6 = host.querySelector('.bag-cell[data-bag-slot="6"]')!;
    mainHand.dispatchEvent(new Event('dragstart', { bubbles: true }));
    bag6.dispatchEvent(new Event('drop', { bubbles: true }));
    expect(unequipCalls[1]).toEqual({ slot: 'main_hand', targetBagSlot: 6 });

    // Right-click stackable ration pack (bag 0, count 4) -> split popover -> first free slot 1, default amount 2 -> confirm.
    bag0.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    fixture.detectChanges();
    const popover = host.querySelector('.split-popover');
    expect(popover).not.toBeNull();
    const splitInput = host.querySelector<HTMLInputElement>('input[name="splitAmount"]')!;
    splitInput.value = '3';
    splitInput.dispatchEvent(new Event('input', { bubbles: true }));
    fixture.detectChanges();
    host.querySelector<HTMLButtonElement>('.split-ok-btn')!.click();
    fixture.detectChanges();
    expect(splitCalls).toEqual([{ fromSlot: 0, toSlot: 1, count: 3 }]);
    expect(host.querySelector('.split-popover')).toBeNull();

    fixture.destroy();
  });

  function hostOpenInventory(fixture: ComponentFixture<HudComponent>): void {
    const host = fixture.nativeElement as HTMLElement;
    host.querySelector<HTMLButtonElement>('.system-inv-btn')!.click();
    fixture.detectChanges();
  }
});

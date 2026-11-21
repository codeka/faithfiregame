import { signal, WritableSignal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { AuthService, SESSION_COOKIE } from '../auth/auth.service';
import { EngineService, GameStatus } from '../engine.service';
import { LoginComponent } from '../login/login.component';
import { HudComponent } from './hud.component';

function clearSessionCookie(): void {
  document.cookie = `${SESSION_COOKIE}=; Path=/; Max-Age=0`;
}

describe('HudComponent & LoginComponent entry flow', () => {
  let gameStatusSig: WritableSignal<GameStatus | null>;
  let editorBuildSig: WritableSignal<boolean>;
  let connectCalls: string[];
  let landingCalls: number;
  let stopPollingCalls: number;
  let dismissPopupCalls: number;
  let origFetch: typeof globalThis.fetch;

  function configureTestBed(): void {
    const mockEngine = {
      ready: signal(true),
      initError: signal<string | null>(null),
      editorBuild: editorBuildSig,
      gameStatus: gameStatusSig,
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
    connectCalls = [];
    landingCalls = 0;
    stopPollingCalls = 0;
    dismissPopupCalls = 0;
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
});

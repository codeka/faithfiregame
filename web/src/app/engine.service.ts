import { Injectable, OnDestroy, signal } from '@angular/core';
import init, { EngineHandle } from './wasm/client_wasm';

@Injectable({ providedIn: 'root' })
export class EngineService implements OnDestroy {
  private handle: EngineHandle | null = null;
  private initializing = false;

  readonly ready = signal(false);
  readonly initError = signal<string | null>(null);

  get rawHandle(): EngineHandle | null {
    return this.handle;
  }

  async init(canvas: HTMLCanvasElement): Promise<void> {
    if (this.handle || this.initializing) {
      return;
    }
    this.initializing = true;
    try {
      await init({ module_or_path: '/wasm/client_wasm_bg.wasm' });
      this.handle = new EngineHandle(canvas);
      this.ready.set(true);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      this.initError.set(msg);
      console.error('Failed to initialize Faith & Fire WASM engine:', err);
    } finally {
      this.initializing = false;
    }
  }

  resize(width: number, height: number): void {
    this.handle?.resize(width, height);
  }

  ngOnDestroy(): void {
    this.handle?.free();
    this.handle = null;
  }
}

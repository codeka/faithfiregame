import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { AreaMetadata, AssetEntry, EngineService } from '../engine.service';

@Injectable()
export class EditorStateService {
  private readonly engine = inject(EngineService);

  readonly areas = signal<AreaMetadata[]>([]);
  readonly catalog = signal<AssetEntry[]>([]);
  readonly selectedAreaId = signal<string>('main');

  readonly brushRadius = signal<number>(4.5);
  readonly brushStrength = signal<number>(3.5);
  readonly selectedAssetId = signal<string>('pine_tree');
  readonly showNewAreaModal = signal<boolean>(false);

  readonly status = this.engine.status;
  readonly activeTool = computed(() => this.status()?.tool ?? 'heightmap');

  private syncedInitialArea = false;

  constructor() {
    effect(() => {
      if (this.engine.ready() && !this.syncedInitialArea && this.areas().length > 0) {
        const mainArea =
          this.areas().find((a) => a.id === this.selectedAreaId()) ?? this.areas()[0];
        if (mainArea) {
          this.syncedInitialArea = true;
          this.selectedAreaId.set(mainArea.id);
          this.engine.setArea(mainArea);
        }
      }
    });
  }

  async initData(): Promise<void> {
    await Promise.all([this.loadAreas(), this.loadCatalog()]);
  }

  async loadAreas(): Promise<void> {
    try {
      const resp = await fetch('/api/editor/areas');
      if (resp.ok) {
        const list = (await resp.json()) as AreaMetadata[];
        this.areas.set(list);
      }
    } catch (e) {
      console.error('Failed to load areas:', e);
    }
  }

  async loadCatalog(): Promise<void> {
    try {
      const resp = await fetch('/world/assets/catalog.json');
      if (resp.ok) {
        const list = (await resp.json()) as AssetEntry[];
        this.catalog.set(list);
        if (list.length > 0 && !list.some((a) => a.id === this.selectedAssetId())) {
          this.selectedAssetId.set(list[0].id);
          this.engine.setDecorationAsset(list[0].id);
        }
      }
    } catch (e) {
      console.error('Failed to load asset catalog:', e);
    }
  }

  selectArea(areaId: string): void {
    this.selectedAreaId.set(areaId);
    const meta = this.areas().find((a) => a.id === areaId);
    if (meta) {
      this.engine.setArea(meta);
    }
  }

  setBrushRadius(val: number): void {
    const r = Number(val);
    this.brushRadius.set(r);
    this.engine.setBrushRadius(r);
  }

  setBrushStrength(val: number): void {
    const s = Number(val);
    this.brushStrength.set(s);
    this.engine.setBrushStrength(s);
  }

  selectAsset(assetId: string): void {
    this.selectedAssetId.set(assetId);
    this.engine.setDecorationAsset(assetId);
  }

  openNewAreaModal(): void {
    this.showNewAreaModal.set(true);
  }

  closeNewAreaModal(): void {
    this.showNewAreaModal.set(false);
  }

  async handleAreaCreated(created: AreaMetadata): Promise<void> {
    await this.loadAreas();
    this.selectedAreaId.set(created.id);
    this.engine.setArea(created);
    this.showNewAreaModal.set(false);
  }
}

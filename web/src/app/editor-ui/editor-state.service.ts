import { computed, effect, inject, Injectable, signal } from '@angular/core';
import { AreaMetadata, AssetEntry, EditorEngineService } from './editor-engine.service';

@Injectable()
export class EditorStateService {
  private readonly engine = inject(EditorEngineService);

  readonly areas = signal<AreaMetadata[]>([]);
  readonly catalog = signal<AssetEntry[]>([]);
  readonly selectedAreaId = signal<string>('main');

  readonly brushRadius = signal<number>(4.5);
  readonly brushStrength = signal<number>(3.5);
  readonly selectedAssetId = signal<string>('pine_tree');
  readonly assetSearch = signal<string>('');
  readonly assetCategory = signal<string>('All');
  readonly showNewAreaModal = signal<boolean>(false);

  readonly status = this.engine.status;
  readonly activeTool = computed(() => this.status()?.tool ?? 'heightmap');

  readonly assetCategories = computed<readonly string[]>(() => {
    const seen = new Set<string>();
    const cats: string[] = ['All'];
    for (const item of this.catalog()) {
      if (item.category && !seen.has(item.category)) {
        seen.add(item.category);
        cats.push(item.category);
      }
    }
    return cats;
  });

  readonly filteredCatalog = computed<readonly AssetEntry[]>(() => {
    const cat = this.assetCategory();
    const tokens = this.assetSearch().trim().toLowerCase().split(/\s+/).filter(Boolean);
    return this.catalog().filter((item) => {
      if (cat !== 'All' && item.category !== cat) {
        return false;
      }
      if (tokens.length === 0) {
        return true;
      }
      const haystack = `${item.name} ${item.category} ${item.id}`.toLowerCase();
      return tokens.every((t) => haystack.includes(t));
    });
  });

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
        if (this.assetCategory() !== 'All' && !list.some((a) => a.category === this.assetCategory())) {
          this.assetCategory.set('All');
        }
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

  setAssetSearch(query: string): void {
    this.assetSearch.set(query);
  }

  setAssetCategory(category: string): void {
    this.assetCategory.set(category);
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

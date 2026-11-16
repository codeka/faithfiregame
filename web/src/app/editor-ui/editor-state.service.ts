import { computed, effect, inject, Injectable, signal } from '@angular/core';
import {
  AreaMetadata,
  AssetCatalog,
  AssetEntry,
  DEFAULT_ROAD_MATERIALS,
  DEFAULT_TEXTURE_LAYERS,
  EditorEngineService,
  RoadMaterialOption,
  RoadTextureEntry,
  TerrainTextureEntry,
  TextureLayerOption,
} from './editor-engine.service';

const DEFAULT_SWATCH_BY_LAYER = new Map<number, string>(
  DEFAULT_TEXTURE_LAYERS.map((layer) => [layer.index, layer.swatch]),
);
const DEFAULT_ROAD_SWATCH_BY_LAYER = new Map<number, string>(
  DEFAULT_ROAD_MATERIALS.map((mat) => [mat.index, mat.swatch]),
);

async function readBlobBytes(blob: Blob): Promise<Uint8Array<ArrayBuffer>> {
  if (typeof blob.arrayBuffer === 'function') {
    return new Uint8Array(await blob.arrayBuffer());
  }
  return new Promise<Uint8Array<ArrayBuffer>>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(new Uint8Array(reader.result as ArrayBuffer));
    reader.onerror = () => reject(reader.error ?? new Error('Failed reading file'));
    reader.readAsArrayBuffer(blob);
  });
}

@Injectable()
export class EditorStateService {
  private readonly engine = inject(EditorEngineService);

  readonly areas = signal<AreaMetadata[]>([]);
  readonly catalog = signal<AssetEntry[]>([]);
  readonly areaAssets = signal<AssetEntry[]>([]);
  readonly terrainCatalog = signal<TerrainTextureEntry[]>([]);
  readonly roadCatalog = signal<RoadTextureEntry[]>([]);
  readonly selectedAreaId = signal<string>('main');
  readonly importingGlb = signal<boolean>(false);
  readonly importMessage = signal<string | null>(null);
  readonly importError = signal<string | null>(null);

  readonly brushRadius = signal<number>(4.5);
  readonly brushStrength = signal<number>(3.5);
  readonly selectedAssetId = signal<string>('pine_tree');
  readonly assetSearch = signal<string>('');
  readonly assetCategory = signal<string>('All');
  readonly textureCategory = signal<string>('All');
  readonly showNewAreaModal = signal<boolean>(false);

  readonly status = this.engine.status;
  readonly activeTool = computed(() => this.status()?.tool ?? 'heightmap');

  readonly terrainTextureLayers = computed<readonly TextureLayerOption[]>(() => {
    const entries = this.terrainCatalog();
    if (entries.length === 0) {
      return DEFAULT_TEXTURE_LAYERS;
    }
    return entries.map((entry, idx) => ({
      index: entry.layer,
      label: entry.name,
      category: entry.category,
      swatch: DEFAULT_SWATCH_BY_LAYER.get(entry.layer) ?? '#6b7280',
      albedoPath: entry.albedo_path || null,
      shortcut: idx < 9 ? String(idx + 1) : '',
    }));
  });

  readonly textureCategories = computed<readonly string[]>(() => {
    const seen = new Set<string>();
    const cats: string[] = ['All'];
    for (const layer of this.terrainTextureLayers()) {
      if (layer.category && !seen.has(layer.category)) {
        seen.add(layer.category);
        cats.push(layer.category);
      }
    }
    return cats;
  });

  readonly roadMaterials = computed<readonly RoadMaterialOption[]>(() => {
    const entries = this.roadCatalog();
    if (entries.length === 0) {
      return DEFAULT_ROAD_MATERIALS;
    }
    return entries.map((entry) => ({
      index: entry.layer,
      label: entry.name,
      edge: entry.edge || 'Curbed',
      swatch: DEFAULT_ROAD_SWATCH_BY_LAYER.get(entry.layer) ?? '#6b7280',
      path: entry.path || null,
    }));
  });

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
          void this.loadAreaAssets(mainArea.id);
        }
      }
    });
  }

  async initData(): Promise<void> {
    await Promise.all([this.loadAreas(), this.loadCatalog()]);
    await this.loadAreaAssets(this.selectedAreaId());
  }

  async loadAreaAssets(areaId: string): Promise<void> {
    if (!areaId) {
      this.areaAssets.set([]);
      return;
    }
    try {
      const resp = await fetch(`/api/editor/areas/${encodeURIComponent(areaId)}/assets`, {
        cache: 'no-cache',
      });
      if (resp.ok) {
        const list = (await resp.json()) as AssetEntry[];
        if (this.selectedAreaId() === areaId) {
          this.areaAssets.set(list);
        }
      }
    } catch (e) {
      console.error('Failed to load area-unique assets:', e);
    }
  }

  async loadAreas(): Promise<void> {
    try {
      const resp = await fetch('/api/editor/areas', { cache: 'no-cache' });
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
      const resp = await fetch('/world/assets/catalog.json', { cache: 'no-cache' });
      if (resp.ok) {
        const raw = (await resp.json()) as AssetCatalog;
        const decorations = raw.decorations ?? [];
        const terrainTextures = raw.terrain_textures ?? [];
        const roadTextures = raw.road_textures ?? [];
        this.catalog.set(decorations);
        this.terrainCatalog.set(terrainTextures);
        this.roadCatalog.set(roadTextures);
        if (
          this.assetCategory() !== 'All' &&
          !decorations.some((a) => a.category === this.assetCategory())
        ) {
          this.assetCategory.set('All');
        }
        if (decorations.length > 0 && !decorations.some((a) => a.id === this.selectedAssetId())) {
          this.selectedAssetId.set(decorations[0].id);
          this.engine.setDecorationAsset(decorations[0].id);
        }
        if (
          this.textureCategory() !== 'All' &&
          !this.textureCategories().includes(this.textureCategory())
        ) {
          this.textureCategory.set('All');
        }
      }
    } catch (e) {
      console.error('Failed to load asset catalog:', e);
    }
  }

  selectArea(areaId: string): void {
    this.selectedAreaId.set(areaId);
    this.importMessage.set(null);
    this.importError.set(null);
    if (this.selectedAssetId().startsWith('local:')) {
      const first = this.catalog()[0];
      if (first) {
        this.selectedAssetId.set(first.id);
      }
    }
    const meta = this.areas().find((a) => a.id === areaId);
    if (meta) {
      this.engine.setArea(meta);
    }
    void this.loadAreaAssets(areaId);
  }

  async importUniqueGlbFiles(files: FileList | readonly File[]): Promise<void> {
    const glbFiles = Array.from(files).filter((f) => f.name.toLowerCase().endsWith('.glb'));
    if (glbFiles.length === 0) {
      this.importError.set('Select one or more binary .glb files.');
      this.importMessage.set(null);
      return;
    }

    const areaId = this.selectedAreaId();
    this.importingGlb.set(true);
    this.importError.set(null);
    this.importMessage.set(null);

    let importedCount = 0;
    try {
      for (const file of glbFiles) {
        const stem = file.name.replace(/\.glb$/i, '');
        const slug =
          stem
            .trim()
            .replace(/[^a-zA-Z0-9-]+/g, '_')
            .replace(/^[_-]+|[_-]+$/g, '')
            .toLowerCase()
            .slice(0, 64) || 'imported_prop';

        const bytes = await readBlobBytes(file);
        const resp = await fetch(
          `/api/editor/areas/${encodeURIComponent(areaId)}/assets/${encodeURIComponent(slug)}`,
          {
            method: 'PUT',
            headers: { 'Content-Type': 'model/gltf-binary' },
            body: file,
          },
        );
        if (!resp.ok) {
          const errText = await resp.text();
          throw new Error(errText || `Upload failed (HTTP ${resp.status})`);
        }
        const entry = (await resp.json()) as AssetEntry;
        const rawSlug = entry.id.startsWith('local:') ? entry.id.slice('local:'.length) : slug;
        this.engine.importLocalGlb(rawSlug, bytes, true);
        importedCount += 1;
      }

      await this.loadAreaAssets(areaId);
      this.importMessage.set(
        importedCount === 1
          ? `Imported ${glbFiles[0].name} into ${areaId}`
          : `Imported ${importedCount} .glb assets into ${areaId}`,
      );
    } catch (e) {
      this.importError.set(e instanceof Error ? e.message : String(e));
    } finally {
      this.importingGlb.set(false);
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

  setTextureCategory(category: string): void {
    this.textureCategory.set(category);
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
    await this.loadAreaAssets(created.id);
    this.showNewAreaModal.set(false);
  }
}

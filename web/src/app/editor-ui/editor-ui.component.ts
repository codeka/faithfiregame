import { Component, computed, effect, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AreaMetadata, AssetEntry, EngineService, formatAreaKind } from '../engine.service';

@Component({
  selector: 'app-editor-ui',
  standalone: true,
  imports: [FormsModule],
  templateUrl: './editor-ui.component.html',
  styleUrl: './editor-ui.component.css',
})
export class EditorUiComponent implements OnInit {
  readonly engine = inject(EngineService);

  readonly areas = signal<AreaMetadata[]>([]);
  readonly catalog = signal<AssetEntry[]>([]);
  readonly selectedAreaId = signal<string>('main');

  // Local slider mirrors synced cleanly with WASM
  readonly brushRadius = signal<number>(4.5);
  readonly brushStrength = signal<number>(3.5);
  readonly selectedAssetId = signal<string>('pine_tree');

  // Create Area Modal state
  readonly showNewAreaModal = signal(false);
  readonly newAreaId = signal('');
  readonly newAreaName = signal('');
  readonly newAreaKind = signal<'outside' | 'inside_ephemeral' | 'inside_permanent'>('outside');
  readonly newAreaWidth = signal(32);
  readonly newAreaLength = signal(32);
  readonly newAreaError = signal<string | null>(null);
  readonly creatingArea = signal(false);

  readonly status = this.engine.status;
  readonly activeTool = computed(() => this.status()?.tool ?? 'heightmap');
  readonly activeToolTitle = computed(() => {
    switch (this.activeTool()) {
      case 'heightmap':
        return 'Heightmap Brush';
      case 'texture':
        return 'Texture Brush';
      case 'decoration':
        return 'Decorations';
    }
  });
  readonly activeHeightMode = computed(() => this.status()?.heightMode ?? 'raise');
  readonly activeTexLayer = computed(() => this.status()?.textureLayer ?? 1);
  readonly activeDecSubmode = computed(() => this.status()?.decorationSubmode ?? 'place');

  readonly activeRotationDeg = computed(() => {
    const s = this.status();
    if (!s) return 0;
    return Math.round(s.selectedProp ? s.selectedProp.rotationDeg : s.previewRotationDeg);
  });

  readonly activeScale = computed(() => {
    const s = this.status();
    if (!s) return 1;
    return Number((s.selectedProp ? s.selectedProp.scale : s.previewScale).toFixed(2));
  });

  readonly textureLayers = [
    { index: 0, label: 'Grass', swatch: '#4a8f3d' },
    { index: 1, label: 'Dirt', swatch: '#8b623f' },
    { index: 2, label: 'Rock', swatch: '#787e88' },
    { index: 3, label: 'Sand', swatch: '#d7be7a' },
  ];

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

  async ngOnInit(): Promise<void> {
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

  describeArea(meta: AreaMetadata): string {
    return `${meta.name} (${formatAreaKind(meta)})`;
  }

  onSelectArea(areaId: string): void {
    this.selectedAreaId.set(areaId);
    const meta = this.areas().find((a) => a.id === areaId);
    if (meta) {
      this.engine.setArea(meta);
    }
  }

  onRadiusChange(val: number): void {
    const r = Number(val);
    this.brushRadius.set(r);
    this.engine.setBrushRadius(r);
  }

  onStrengthChange(val: number): void {
    const s = Number(val);
    this.brushStrength.set(s);
    this.engine.setBrushStrength(s);
  }

  onAssetChange(assetId: string): void {
    this.selectedAssetId.set(assetId);
    this.engine.setDecorationAsset(assetId);
  }

  onRotationChange(deg: number): void {
    this.engine.setPropRotationDeg(Number(deg));
  }

  onScaleChange(scale: number): void {
    this.engine.setPropScale(Number(scale));
  }

  openNewAreaModal(): void {
    this.newAreaId.set('');
    this.newAreaName.set('');
    this.newAreaKind.set('outside');
    this.newAreaWidth.set(32);
    this.newAreaLength.set(32);
    this.newAreaError.set(null);
    this.showNewAreaModal.set(true);
  }

  async createNewArea(): Promise<void> {
    this.newAreaError.set(null);
    const id = this.newAreaId().trim().toLowerCase();
    const name = this.newAreaName().trim();
    if (!id || !name) {
      this.newAreaError.set('Area ID and Name are required.');
      return;
    }

    this.creatingArea.set(true);
    try {
      const kind = this.newAreaKind();
      const body: Record<string, unknown> = { id, name, kind };
      if (kind !== 'outside') {
        body['width'] = Number(this.newAreaWidth());
        body['length'] = Number(this.newAreaLength());
      }

      const resp = await fetch('/api/editor/areas', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (!resp.ok) {
        const text = await resp.text();
        this.newAreaError.set(text || `HTTP ${resp.status}`);
        return;
      }

      const created = (await resp.json()) as AreaMetadata;
      await this.loadAreas();
      this.selectedAreaId.set(created.id);
      this.engine.setArea(created);
      this.showNewAreaModal.set(false);
    } catch (e) {
      this.newAreaError.set(e instanceof Error ? e.message : String(e));
    } finally {
      this.creatingArea.set(false);
    }
  }
}

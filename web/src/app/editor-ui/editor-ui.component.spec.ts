import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { EditorEngineService, EditorStatus } from './editor-engine.service';
import { EditorUiComponent } from './editor-ui.component';

function baseStatus(tool: EditorStatus['tool'] = 'decoration'): EditorStatus {
  return {
    areaId: 'main',
    areaName: 'Main World',
    isOutside: true,
    currentTile: [0, 0],
    dimensions: null,
    cursorPos: null,
    dirtyCount: 0,
    saveState: 'Saved',
    tool,
    heightMode: 'raise',
    textureLayer: 0,
    decorationSubmode: 'place',
    selectedProp: null,
    previewRotationDeg: [0, 0, 0],
    previewScale: 1,
    previewCastShadow: true,
  };
}

describe('EditorUiComponent number-key mode shortcuts', () => {
  let fixture: ComponentFixture<EditorUiComponent>;
  let component: EditorUiComponent;
  let statusSig = signal<EditorStatus | null>(baseStatus('decoration'));
  let heightCalls: string[];
  let textureCalls: number[];
  let decorationCalls: string[];
  let duplicateCalls: number;
  let origFetch: typeof globalThis.fetch;

  beforeEach(async () => {
    origFetch = globalThis.fetch;
    globalThis.fetch = (async () =>
      new Response('[]', {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })) as typeof fetch;

    statusSig = signal<EditorStatus | null>(baseStatus('decoration'));
    heightCalls = [];
    textureCalls = [];
    decorationCalls = [];
    duplicateCalls = 0;

    const mockEngine = {
      ready: signal(false),
      editorEnabled: signal(true),
      initError: signal<string | null>(null),
      status: statusSig,
      setArea: () => {},
      setTool: (tool: EditorStatus['tool']) => {
        statusSig.update((s) => (s ? { ...s, tool } : s));
      },
      setHeightMode: (mode: EditorStatus['heightMode']) => {
        heightCalls.push(mode);
        statusSig.update((s) => (s ? { ...s, heightMode: mode } : s));
      },
      setBrushRadius: () => {},
      setBrushStrength: () => {},
      setTextureLayer: (layer: number) => {
        textureCalls.push(layer);
        statusSig.update((s) => (s ? { ...s, textureLayer: layer } : s));
      },
      setDecorationSubmode: (submode: EditorStatus['decorationSubmode']) => {
        decorationCalls.push(submode);
        statusSig.update((s) => (s ? { ...s, decorationSubmode: submode } : s));
      },
      snapSelectedPropToTerrain: () => {},
      setDecorationAsset: () => {},
      setPropRotationAxisDeg: () => {},
      resetPropRotation: () => {},
      setPropScale: () => {},
      setPropCastShadow: () => {},
      duplicateSelectedProp: () => {
        duplicateCalls++;
      },
      deleteSelectedProp: () => {},
      saveNow: () => {},
    };

    await TestBed.configureTestingModule({
      imports: [EditorUiComponent],
      providers: [{ provide: EditorEngineService, useValue: mockEngine }],
    }).compileComponents();

    fixture = TestBed.createComponent(EditorUiComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  afterEach(() => {
    globalThis.fetch = origFetch;
  });

  it('switches decoration submodes with keys 1-4', () => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));

    expect(decorationCalls).toEqual(['move', 'free_move', 'rotate', 'place']);
  });

  it('switches heightmap sculpt modes and texture layers with keys 1-4', () => {
    statusSig.set(baseStatus('heightmap'));
    fixture.detectChanges();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));

    expect(heightCalls).toEqual(['raise', 'lower', 'smooth', 'flatten']);

    statusSig.set(baseStatus('texture'));
    fixture.detectChanges();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));

    expect(textureCalls).toEqual([0, 1, 2, 3]);
  });

  it('ignores text/number inputs, modifiers, and open modal, while allowing range sliders', () => {
    const textInput = document.createElement('input');
    textInput.type = 'text';
    document.body.appendChild(textInput);
    textInput.dispatchEvent(new KeyboardEvent('keydown', { key: '2', bubbles: true }));
    textInput.remove();

    const numInput = document.createElement('input');
    numInput.type = 'number';
    document.body.appendChild(numInput);
    numInput.dispatchEvent(new KeyboardEvent('keydown', { key: '3', bubbles: true }));
    numInput.remove();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2', ctrlKey: true }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3', shiftKey: true }));

    component.state.openNewAreaModal();
    fixture.detectChanges();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));
    expect(decorationCalls).toEqual([]);

    component.state.closeNewAreaModal();
    fixture.detectChanges();

    const rangeInput = document.createElement('input');
    rangeInput.type = 'range';
    document.body.appendChild(rangeInput);
    rangeInput.dispatchEvent(new KeyboardEvent('keydown', { key: '3', bubbles: true }));
    rangeInput.remove();

    expect(decorationCalls).toEqual(['free_move']);
  });

  it('shows Asset list only in Place mode and Transform controls only in Move/Free Move/Rotate modes', () => {
    const root = fixture.nativeElement as HTMLElement;

    // 1. Place mode: Asset list present, Transform controls, Duplicate Selected & Delete Selected absent
    expect(root.querySelector('.asset-list')).not.toBeNull();
    expect(root.querySelector('input[name="propRotationX"]')).toBeNull();
    expect(root.querySelector('input[name="propScale"]')).toBeNull();
    expect(root.querySelector('input[name="propCastShadow"]')).toBeNull();
    expect(root.querySelector('.btn-duplicate')).toBeNull();
    expect(root.querySelector('.btn-danger')).toBeNull();

    // 2. Non-Place modes with no selection: Asset list absent, Transform controls rendered & disabled
    for (const key of ['2', '3', '4']) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
      fixture.detectChanges();

      expect(root.querySelector('.asset-list')).toBeNull();
      const rotX = root.querySelector('input[name="propRotationX"]') as HTMLInputElement | null;
      const scale = root.querySelector('input[name="propScale"]') as HTMLInputElement | null;
      const shadow = root.querySelector('input[name="propCastShadow"]') as HTMLInputElement | null;
      const dupBtn = root.querySelector('.btn-duplicate') as HTMLButtonElement | null;
      const delBtn = root.querySelector('.btn-danger') as HTMLButtonElement | null;

      expect(rotX?.disabled).toBe(true);
      expect(scale?.disabled).toBe(true);
      expect(shadow?.disabled).toBe(true);
      expect(dupBtn?.disabled).toBe(true);
      expect(delBtn?.disabled).toBe(true);
    }

    // 3. Non-Place mode with a selected prop: Transform controls & Duplicate/Delete enabled above Delete
    statusSig.update((s) =>
      s
        ? {
            ...s,
            selectedProp: {
              id: 1001,
              assetId: 'rock_boulder',
              position: [10, 2, 15],
              rotationDeg: [15, 90, 0],
              scale: 1.5,
              castShadow: false,
            },
          }
        : s,
    );
    fixture.detectChanges();

    expect(root.querySelector('.asset-list')).toBeNull();
    expect((root.querySelector('input[name="propRotationX"]') as HTMLInputElement).disabled).toBe(false);
    expect((root.querySelector('input[name="propScale"]') as HTMLInputElement).disabled).toBe(false);
    expect((root.querySelector('input[name="propCastShadow"]') as HTMLInputElement).disabled).toBe(false);
    const dupBtn = root.querySelector('.btn-duplicate') as HTMLButtonElement;
    const delBtn = root.querySelector('.btn-danger') as HTMLButtonElement;
    expect(dupBtn.disabled).toBe(false);
    expect(delBtn.disabled).toBe(false);
    expect(dupBtn.nextElementSibling).toBe(delBtn);

    dupBtn.click();
    expect(duplicateCalls).toBe(1);

    // 4. Returning to Place mode restores Asset list, shows preview yaw hint, and removes Transform controls even when a prop is selected
    statusSig.update((s) => (s ? { ...s, previewRotationDeg: [0, 135.4, 0] } : s));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    fixture.detectChanges();

    expect(root.querySelector('.asset-list')).not.toBeNull();
    expect(root.querySelector('.toolbox-hint')?.textContent).toContain('Q/E: Rotate (135°)');
    expect(root.querySelector('input[name="propRotationX"]')).toBeNull();
    expect(root.querySelector('input[name="propScale"]')).toBeNull();
    expect(root.querySelector('input[name="propCastShadow"]')).toBeNull();
    expect(root.querySelector('.btn-duplicate')).toBeNull();
    expect(root.querySelector('.btn-danger')).toBeNull();
  });

  it('filters the asset palette by category pill and search query while suppressing shortcuts inside search', async () => {
    const mockCatalog = [
      { id: 'pine_tree', name: 'Pine Tree', path: '/world/assets/decorations/pine_tree.glb', category: 'Nature' },
      { id: 'rock_boulder', name: 'Boulder', path: '/world/assets/decorations/rock_boulder.glb', category: 'Rocks' },
      { id: 'cliff_wall', name: 'Cliff Wall', path: '/world/assets/decorations/cliff_wall.glb', category: 'Rocks' },
      { id: 'cliff_tall_pillar', name: 'Tall Cliff Pillar', path: '/world/assets/decorations/cliff_tall_pillar.glb', category: 'Rocks' },
      { id: 'ruin_stone', name: 'Ruin Stone', path: '/world/assets/decorations/ruin_stone.glb', category: 'Props' },
    ];
    globalThis.fetch = (async () =>
      new Response(JSON.stringify(mockCatalog), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })) as typeof fetch;

    await component.state.loadCatalog();
    fixture.detectChanges();

    const root = fixture.nativeElement as HTMLElement;
    const assetNames = () =>
      Array.from(root.querySelectorAll('.asset-btn')).map((b) => b.firstElementChild?.textContent?.trim());

    expect(assetNames()).toEqual(['Pine Tree', 'Boulder', 'Cliff Wall', 'Tall Cliff Pillar', 'Ruin Stone']);

    // Category pill filtering
    const pills = Array.from(root.querySelectorAll('.pill-btn')) as HTMLButtonElement[];
    expect(pills.map((p) => p.textContent?.trim())).toEqual(['All', 'Nature', 'Rocks', 'Props']);
    pills.find((p) => p.textContent?.trim() === 'Rocks')?.click();
    fixture.detectChanges();
    expect(assetNames()).toEqual(['Boulder', 'Cliff Wall', 'Tall Cliff Pillar']);

    // Search input filtering + hotkey suppression while typing
    const searchInput = root.querySelector('input[name="assetSearch"]') as HTMLInputElement;
    expect(searchInput).not.toBeNull();
    searchInput.dispatchEvent(new KeyboardEvent('keydown', { key: '2', bubbles: true }));
    expect(decorationCalls).toEqual([]);

    searchInput.value = 'cliff';
    searchInput.dispatchEvent(new Event('input', { bubbles: true }));
    fixture.detectChanges();
    expect(assetNames()).toEqual(['Cliff Wall', 'Tall Cliff Pillar']);

    // Empty search match
    searchInput.value = 'zzzz';
    searchInput.dispatchEvent(new Event('input', { bubbles: true }));
    fixture.detectChanges();
    expect(assetNames()).toEqual([]);
    expect(root.querySelector('.empty-assets')?.textContent?.trim()).toBe('No matching assets');

    // Clear search button restores category-filtered items
    (root.querySelector('.search-clear') as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(assetNames()).toEqual(['Boulder', 'Cliff Wall', 'Tall Cliff Pillar']);

    // Filter persists across switching to Rotate mode and back to Place
    searchInput.value = 'pillar';
    searchInput.dispatchEvent(new Event('input', { bubbles: true }));
    fixture.detectChanges();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));
    fixture.detectChanges();
    expect(root.querySelector('.asset-list')).toBeNull();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    fixture.detectChanges();
    expect(assetNames()).toEqual(['Tall Cliff Pillar']);
  });
});

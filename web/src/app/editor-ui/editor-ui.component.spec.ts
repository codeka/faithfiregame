import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { EditorStatus, EngineService } from '../engine.service';
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
      deleteSelectedProp: () => {},
      saveNow: () => {},
    };

    await TestBed.configureTestingModule({
      imports: [EditorUiComponent],
      providers: [{ provide: EngineService, useValue: mockEngine }],
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

    // 1. Place mode: Asset list present, Transform controls & Delete Selected absent
    expect(root.querySelector('.asset-list')).not.toBeNull();
    expect(root.querySelector('input[name="propRotationX"]')).toBeNull();
    expect(root.querySelector('input[name="propScale"]')).toBeNull();
    expect(root.querySelector('input[name="propCastShadow"]')).toBeNull();
    expect(root.querySelector('.btn-danger')).toBeNull();

    // 2. Non-Place modes with no selection: Asset list absent, Transform controls rendered & disabled
    for (const key of ['2', '3', '4']) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
      fixture.detectChanges();

      expect(root.querySelector('.asset-list')).toBeNull();
      const rotX = root.querySelector('input[name="propRotationX"]') as HTMLInputElement | null;
      const scale = root.querySelector('input[name="propScale"]') as HTMLInputElement | null;
      const shadow = root.querySelector('input[name="propCastShadow"]') as HTMLInputElement | null;
      const delBtn = root.querySelector('.btn-danger') as HTMLButtonElement | null;

      expect(rotX?.disabled).toBe(true);
      expect(scale?.disabled).toBe(true);
      expect(shadow?.disabled).toBe(true);
      expect(delBtn?.disabled).toBe(true);
    }

    // 3. Non-Place mode with a selected prop: Transform controls enabled, Asset list still absent
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
    expect((root.querySelector('.btn-danger') as HTMLButtonElement).disabled).toBe(false);

    // 4. Returning to Place mode restores Asset list and removes Transform controls even when a prop is selected
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    fixture.detectChanges();

    expect(root.querySelector('.asset-list')).not.toBeNull();
    expect(root.querySelector('input[name="propRotationX"]')).toBeNull();
    expect(root.querySelector('input[name="propScale"]')).toBeNull();
    expect(root.querySelector('input[name="propCastShadow"]')).toBeNull();
    expect(root.querySelector('.btn-danger')).toBeNull();
  });
});

import { signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import {
  CharacterCatalog,
  CharacterDefinition,
  EditorEngineService,
  EditorMode,
  EditorStatus,
} from './editor-engine.service';
import { EditorUiComponent } from './editor-ui.component';

const MOCK_CHARACTER_CATALOG: CharacterCatalog = {
  skeletons: [
    {
      id: 'humanoid',
      name: 'Humanoid',
      rig_path: '/world/assets/characters/human_base_body.glb',
      render_base_mesh: true,
      skinned_slots: [
        { id: 'head', name: 'Head' },
        { id: 'torso', name: 'Torso' },
        { id: 'legs', name: 'Legs' },
      ],
      sockets: [
        {
          id: 'socket_head',
          name: 'Head Socket',
          parent_bone: 'head',
          offset: [0, 0, 0],
          rotation_deg: [0, 0, 0],
          scale: 1,
        },
        {
          id: 'socket_hand_r',
          name: 'Right Hand',
          parent_bone: 'hand_r',
          offset: [0, -0.06, 0.04],
          rotation_deg: [0, 0, 0],
          scale: 1,
        },
      ],
      animation_paths: [],
    },
  ],
  parts: [
    {
      id: 'head_standard',
      name: 'Standard Head',
      skeleton_id: 'humanoid',
      attachment: { kind: 'skinned', slot: 'head' },
      path: '/world/assets/characters/head_standard.glb',
      category: 'Head',
    },
    {
      id: 'head_aug_visor',
      name: 'Augmented Visor Head',
      skeleton_id: 'humanoid',
      attachment: { kind: 'skinned', slot: 'head' },
      path: '/world/assets/characters/head_aug_visor.glb',
      category: 'Head',
    },
    {
      id: 'torso_cryo_jumpsuit',
      name: 'Cryo Jumpsuit',
      skeleton_id: 'humanoid',
      attachment: { kind: 'skinned', slot: 'torso' },
      path: '/world/assets/characters/torso_cryo_jumpsuit.glb',
      category: 'Torso',
    },
    {
      id: 'torso_faith_tunic',
      name: 'Faith Tunic',
      skeleton_id: 'humanoid',
      attachment: { kind: 'skinned', slot: 'torso' },
      path: '/world/assets/characters/torso_faith_tunic.glb',
      category: 'Torso',
    },
    {
      id: 'legs_cargo_pants',
      name: 'Cargo Pants',
      skeleton_id: 'humanoid',
      attachment: { kind: 'skinned', slot: 'legs' },
      path: '/world/assets/characters/legs_cargo_pants.glb',
      category: 'Legs',
    },
    {
      id: 'hair_short_crop',
      name: 'Short Crop Hair',
      skeleton_id: 'humanoid',
      attachment: {
        kind: 'socket',
        socket_id: 'socket_head',
        offset: [0, 0, 0],
        rotation_deg: [0, 0, 0],
        scale: 1,
      },
      path: '/world/assets/characters/hair_short_crop.glb',
      category: 'Hair',
    },
    {
      id: 'weapon_guard_rifle',
      name: 'Guard Carbine',
      skeleton_id: 'humanoid',
      attachment: {
        kind: 'socket',
        socket_id: 'socket_hand_r',
        offset: [0, 0, 0],
        rotation_deg: [0, 0, 0],
        scale: 1,
      },
      path: '/world/assets/characters/weapon_guard_rifle.glb',
      category: 'Weapons',
    },
  ],
};

const MOCK_CHARACTER_PRESETS: CharacterDefinition[] = [
  {
    id: 'cryo_prisoner',
    name: 'Cryo Prisoner',
    skeleton_id: 'humanoid',
    show_base_mesh: true,
    skinned_parts: {
      head: 'head_standard',
      torso: 'torso_cryo_jumpsuit',
      legs: 'legs_cargo_pants',
    },
    socket_parts: {
      socket_head: 'hair_short_crop',
    },
    palette: {},
  },
  {
    id: 'fire_raider',
    name: 'Fire Raider',
    skeleton_id: 'humanoid',
    show_base_mesh: true,
    skinned_parts: {
      head: 'head_aug_visor',
      torso: 'torso_faith_tunic',
      legs: 'legs_cargo_pants',
    },
    socket_parts: {
      socket_hand_r: 'weapon_guard_rifle',
    },
    palette: {
      primary_color: [180, 40, 30],
    },
  },
];

function baseStatus(tool: EditorStatus['tool'] = 'decoration'): EditorStatus {
  return {
    editorMode: 'world',
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
    roadSubmode: 'place',
    roadStyle: 'curve',
    roadMaterial: 1,
    roadWidth: 4,
    roadShoulder: 3,
    roadDraftPoints: 0,
    selectedRoad: null,
    decorationSubmode: 'place',
    selectedProp: null,
    previewRotationDeg: [0, 0, 0],
    previewScale: 1,
    previewCastShadow: true,
    walkabilityMode: 'block',
    characterStudio: {
      activePresetId: 'cryo_prisoner',
      activeSkeletonId: 'humanoid',
      availableClips: ['Idle', 'Walk', 'Run'],
      activeClip: 'Idle',
      playing: true,
      animTime: 0.25,
      clipDuration: 2,
      playbackSpeed: 1,
      showSkeletonDebug: false,
      turntable: false,
      selectedSocketId: 'socket_head',
      boneNames: ['root', 'pelvis', 'spine_01', 'spine_02', 'neck', 'head', 'hand_r'],
      saveState: 'Saved',
    },
  };
}

describe('EditorUiComponent number-key mode shortcuts', () => {
  let fixture: ComponentFixture<EditorUiComponent>;
  let component: EditorUiComponent;
  let statusSig = signal<EditorStatus | null>(baseStatus('decoration'));
  let heightCalls: string[];
  let textureCalls: number[];
  let roadSubmodeCalls: string[];
  let roadStyleCalls: string[];
  let roadMaterialCalls: number[];
  let roadWidthCalls: number[];
  let roadShoulderCalls: number[];
  let finishRoadCalls: number;
  let cancelRoadCalls: number;
  let relevelRoadCalls: number;
  let deleteRoadNodeCalls: number;
  let deleteRoadCalls: number;
  let decorationCalls: string[];
  let walkabilityModeCalls: string[];
  let bakeWalkCalls: number;
  let resetWalkCalls: number;
  let importedLocalGlbs: { slug: string; byteLen: number; spawn: boolean }[];
  let duplicateCalls: number;
  let modeCalls: EditorMode[];
  let activeCharactersPushed: CharacterDefinition[];
  let catalogsPushed: CharacterCatalog[];
  let loadedCharGlbs: { path: string; byteLen: number }[];
  let animClipCalls: string[];
  let animPlayingCalls: boolean[];
  let animSpeedCalls: number[];
  let debugSkeletonCalls: boolean[];
  let origFetch: typeof globalThis.fetch;

  beforeEach(async () => {
    origFetch = globalThis.fetch;
    globalThis.fetch = (async (input: RequestInfo | URL) => {
      const url = String(input);
      let body = '[]';
      if (url.includes('/world/assets/catalog.json')) {
        body = JSON.stringify({ decorations: [], terrain_textures: [], road_textures: [] });
      } else if (url.includes('/api/editor/characters/catalog')) {
        body = JSON.stringify(MOCK_CHARACTER_CATALOG);
      } else if (url.includes('/api/editor/characters/presets')) {
        body = JSON.stringify(MOCK_CHARACTER_PRESETS);
      }
      return new Response(body, {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    }) as typeof fetch;

    statusSig = signal<EditorStatus | null>(baseStatus('decoration'));
    heightCalls = [];
    textureCalls = [];
    roadSubmodeCalls = [];
    roadStyleCalls = [];
    roadMaterialCalls = [];
    roadWidthCalls = [];
    roadShoulderCalls = [];
    finishRoadCalls = 0;
    cancelRoadCalls = 0;
    relevelRoadCalls = 0;
    deleteRoadNodeCalls = 0;
    deleteRoadCalls = 0;
    decorationCalls = [];
    walkabilityModeCalls = [];
    bakeWalkCalls = 0;
    resetWalkCalls = 0;
    importedLocalGlbs = [];
    duplicateCalls = 0;
    modeCalls = [];
    activeCharactersPushed = [];
    catalogsPushed = [];
    loadedCharGlbs = [];
    animClipCalls = [];
    animPlayingCalls = [];
    animSpeedCalls = [];
    debugSkeletonCalls = [];

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
      setRoadSubmode: (submode: EditorStatus['roadSubmode']) => {
        roadSubmodeCalls.push(submode);
        statusSig.update((s) => (s ? { ...s, roadSubmode: submode } : s));
      },
      setRoadStyle: (style: EditorStatus['roadStyle']) => {
        roadStyleCalls.push(style);
        statusSig.update((s) => (s ? { ...s, roadStyle: style } : s));
      },
      setRoadMaterial: (material: number) => {
        roadMaterialCalls.push(material);
        statusSig.update((s) => (s ? { ...s, roadMaterial: material } : s));
      },
      setRoadWidth: (width: number) => {
        roadWidthCalls.push(width);
        statusSig.update((s) => (s ? { ...s, roadWidth: width } : s));
      },
      setRoadShoulder: (shoulder: number) => {
        roadShoulderCalls.push(shoulder);
        statusSig.update((s) => (s ? { ...s, roadShoulder: shoulder } : s));
      },
      finishActiveRoad: () => {
        finishRoadCalls++;
      },
      cancelActiveRoad: () => {
        cancelRoadCalls++;
      },
      relevelSelectedRoad: () => {
        relevelRoadCalls++;
      },
      deleteSelectedRoadNode: () => {
        deleteRoadNodeCalls++;
      },
      deleteSelectedRoad: () => {
        deleteRoadCalls++;
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
      setWalkabilityMode: (mode: EditorStatus['walkabilityMode']) => {
        walkabilityModeCalls.push(mode);
        statusSig.update((s) => (s ? { ...s, walkabilityMode: mode } : s));
      },
      bakePropsToWalkability: () => {
        bakeWalkCalls++;
      },
      resetWalkabilityToSlope: () => {
        resetWalkCalls++;
      },
      importLocalGlb: (slug: string, bytes: Uint8Array, spawnInScene = true) => {
        importedLocalGlbs.push({ slug, byteLen: bytes.byteLength, spawn: spawnInScene });
        return `local:${slug}`;
      },
      saveNow: () => {},
      setEditorMode: (mode: EditorMode) => {
        modeCalls.push(mode);
        statusSig.update((s) => (s ? { ...s, editorMode: mode } : s));
      },
      setCharacterCatalog: (cat: CharacterCatalog) => {
        catalogsPushed.push(structuredClone(cat));
      },
      setActiveCharacter: (def: CharacterDefinition) => {
        activeCharactersPushed.push(structuredClone(def));
        statusSig.update((s) =>
          s
            ? {
                ...s,
                characterStudio: {
                  ...s.characterStudio,
                  activePresetId: def.id,
                  activeSkeletonId: def.skeleton_id,
                },
              }
            : s,
        );
      },
      loadCharacterGlbBytes: (path: string, bytes: Uint8Array) => {
        loadedCharGlbs.push({ path, byteLen: bytes.byteLength });
      },
      setCharacterAnimClip: (clip: string) => {
        animClipCalls.push(clip);
        statusSig.update((s) =>
          s
            ? {
                ...s,
                characterStudio: {
                  ...s.characterStudio,
                  activeClip: clip === '__bind__' ? null : clip,
                  animTime: 0,
                },
              }
            : s,
        );
      },
      setCharacterAnimPlaying: (playing: boolean) => {
        animPlayingCalls.push(playing);
        statusSig.update((s) =>
          s ? { ...s, characterStudio: { ...s.characterStudio, playing } } : s,
        );
      },
      setCharacterAnimTime: (timeSec: number) => {
        statusSig.update((s) =>
          s ? { ...s, characterStudio: { ...s.characterStudio, animTime: timeSec } } : s,
        );
      },
      setCharacterAnimSpeed: (speed: number) => {
        animSpeedCalls.push(speed);
        statusSig.update((s) =>
          s ? { ...s, characterStudio: { ...s.characterStudio, playbackSpeed: speed } } : s,
        );
      },
      setCharacterDebugSkeleton: (show: boolean) => {
        debugSkeletonCalls.push(show);
        statusSig.update((s) =>
          s
            ? {
                ...s,
                characterStudio: { ...s.characterStudio, showSkeletonDebug: show },
              }
            : s,
        );
      },
      setCharacterTurntable: (enabled: boolean) => {
        statusSig.update((s) =>
          s ? { ...s, characterStudio: { ...s.characterStudio, turntable: enabled } } : s,
        );
      },
      setSelectedSocketId: (socketId: string | null) => {
        statusSig.update((s) =>
          s
            ? {
                ...s,
                characterStudio: { ...s.characterStudio, selectedSocketId: socketId },
              }
            : s,
        );
      },
      resetCharacterStudioCamera: () => {},
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

    const matButtons = (fixture.nativeElement as HTMLElement).querySelectorAll('.material-btn');
    expect(matButtons.length).toBe(9);

    for (const key of ['1', '2', '3', '4', '5', '6', '7', '8', '9']) {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    }

    expect(textureCalls).toEqual([0, 4, 5, 6, 1, 3, 2, 7, 8]);
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
    const mockCatalog = {
      decorations: [
        { id: 'pine_tree', name: 'Pine Tree', path: '/world/assets/decorations/pine_tree.glb', category: 'Nature' },
        { id: 'rock_boulder', name: 'Boulder', path: '/world/assets/decorations/rock_boulder.glb', category: 'Rocks' },
        { id: 'cliff_wall', name: 'Cliff Wall', path: '/world/assets/decorations/cliff_wall.glb', category: 'Rocks' },
        { id: 'cliff_tall_pillar', name: 'Tall Cliff Pillar', path: '/world/assets/decorations/cliff_tall_pillar.glb', category: 'Rocks' },
        { id: 'ruin_stone', name: 'Ruin Stone', path: '/world/assets/decorations/ruin_stone.glb', category: 'Props' },
      ],
      terrain_textures: [],
      road_textures: [],
    };
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

  it('groups texture materials by category pills while preserving internal material indices', () => {
    statusSig.set(baseStatus('texture'));
    fixture.detectChanges();

    const root = fixture.nativeElement as HTMLElement;
    const matNames = () =>
      Array.from(root.querySelectorAll('.material-btn .material-main span:last-child')).map((s) =>
        s.textContent?.trim(),
      );
    const pills = () => Array.from(root.querySelectorAll('.pill-btn')) as HTMLButtonElement[];

    expect(pills().map((p) => p.textContent?.trim())).toEqual(['All', 'Grass', 'Earth', 'Stone']);
    expect(matNames()).toEqual([
      'Grass',
      'Light Grass',
      'Dark Grass',
      'Patchy Grass',
      'Dirt',
      'Sand',
      'Rock',
      'Cobblestone',
      'Brick',
    ]);
    expect(root.querySelector('.material-count')?.textContent?.trim()).toBe('9');

    // Filter to Grass and click Light Grass (internal index 4)
    pills().find((p) => p.textContent?.trim() === 'Grass')?.click();
    fixture.detectChanges();
    expect(matNames()).toEqual(['Grass', 'Light Grass', 'Dark Grass', 'Patchy Grass']);
    expect(root.querySelector('.material-count')?.textContent?.trim()).toBe('4 / 9');
    (root.querySelectorAll('.material-btn')[1] as HTMLButtonElement).click();
    expect(textureCalls.at(-1)).toBe(4);

    // Filter to Earth and click Dirt (internal index 1)
    pills().find((p) => p.textContent?.trim() === 'Earth')?.click();
    fixture.detectChanges();
    expect(matNames()).toEqual(['Dirt', 'Sand']);
    expect(root.querySelector('.material-count')?.textContent?.trim()).toBe('2 / 9');
    (root.querySelectorAll('.material-btn')[0] as HTMLButtonElement).click();
    expect(textureCalls.at(-1)).toBe(1);

    // Filter to Stone, click Cobblestone (internal index 7), verify persistence across tool switch
    pills().find((p) => p.textContent?.trim() === 'Stone')?.click();
    fixture.detectChanges();
    expect(matNames()).toEqual(['Rock', 'Cobblestone', 'Brick']);
    expect(root.querySelector('.material-count')?.textContent?.trim()).toBe('3 / 9');
    (root.querySelectorAll('.material-btn')[1] as HTMLButtonElement).click();
    expect(textureCalls.at(-1)).toBe(7);

    statusSig.set(baseStatus('heightmap'));
    fixture.detectChanges();
    statusSig.set(baseStatus('texture'));
    fixture.detectChanges();
    expect(matNames()).toEqual(['Rock', 'Cobblestone', 'Brick']);
  });

  it('orders Roads between Texture and Decoration and dispatches road shortcuts and controls', () => {
    const root = fixture.nativeElement as HTMLElement;
    const primaryBtns = Array.from(
      root.querySelectorAll('app-editor-toolbar .group:nth-of-type(3) button'),
    ) as HTMLButtonElement[];
    expect(primaryBtns.map((b) => b.textContent?.trim())).toEqual([
      'Heightmap',
      'Texture',
      'Roads',
      'Decoration',
      'Walkability',
    ]);

    primaryBtns[2].click();
    fixture.detectChanges();
    expect(statusSig()?.tool).toBe('road');
    expect(root.querySelector('app-road-tool')).not.toBeNull();

    // Hotkeys 1 and 2 toggle Place / Edit; 3-9 are ignored in Road tool
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    expect(roadSubmodeCalls).toEqual(['edit', 'place']);
    fixture.detectChanges();

    // Place mode controls: style, surface, profile sliders, draft Finish / Cancel
    const styleBtns = Array.from(root.querySelectorAll('.road-style-btn')) as HTMLButtonElement[];
    expect(styleBtns.map((b) => b.textContent?.trim())).toEqual(['Curve', 'Straight']);
    styleBtns[1].click();
    expect(roadStyleCalls).toEqual(['straight']);

    const matBtns = Array.from(root.querySelectorAll('.road-mat-btn')) as HTMLButtonElement[];
    expect(matBtns.length).toBe(5);
    matBtns[3].click();
    expect(roadMaterialCalls).toEqual([3]);

    const widthInput = root.querySelector('input[name="roadWidth"]') as HTMLInputElement;
    widthInput.value = '6';
    widthInput.dispatchEvent(new Event('input', { bubbles: true }));
    const shoulderInput = root.querySelector('input[name="roadShoulder"]') as HTMLInputElement;
    shoulderInput.value = '4.5';
    shoulderInput.dispatchEvent(new Event('input', { bubbles: true }));
    expect(roadWidthCalls).toEqual([6]);
    expect(roadShoulderCalls).toEqual([4.5]);

    const finishBtn = () => root.querySelector('.road-finish-btn') as HTMLButtonElement;
    const cancelBtn = () => root.querySelector('.road-cancel-btn') as HTMLButtonElement;
    expect(finishBtn().disabled).toBe(true);
    expect(cancelBtn().disabled).toBe(true);

    statusSig.update((s) => (s ? { ...s, roadDraftPoints: 1 } : s));
    fixture.detectChanges();
    expect(finishBtn().disabled).toBe(true);
    expect(cancelBtn().disabled).toBe(false);

    statusSig.update((s) => (s ? { ...s, roadDraftPoints: 3 } : s));
    fixture.detectChanges();
    expect(finishBtn().disabled).toBe(false);
    finishBtn().click();
    cancelBtn().click();
    expect(finishRoadCalls).toBe(1);
    expect(cancelRoadCalls).toBe(1);

    // Edit mode: disabled without selection, enabled with selected road/node
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    fixture.detectChanges();
    expect((root.querySelector('.road-relevel-btn') as HTMLButtonElement).disabled).toBe(true);
    expect((root.querySelector('.road-del-node-btn') as HTMLButtonElement).disabled).toBe(true);
    expect((root.querySelector('.road-del-btn') as HTMLButtonElement).disabled).toBe(true);
    expect((root.querySelector('.road-style-btn') as HTMLButtonElement).disabled).toBe(true);

    statusSig.update((s) =>
      s
        ? {
            ...s,
            selectedRoad: {
              id: 1004,
              style: 'straight',
              material: 2,
              width: 5,
              shoulder: 3.5,
              pointCount: 3,
              selectedNode: 1,
            },
          }
        : s,
    );
    fixture.detectChanges();

    const relevelBtn = root.querySelector('.road-relevel-btn') as HTMLButtonElement;
    const delNodeBtn = root.querySelector('.road-del-node-btn') as HTMLButtonElement;
    const delRoadBtn = root.querySelector('.road-del-btn') as HTMLButtonElement;
    expect(relevelBtn.disabled).toBe(false);
    expect(delNodeBtn.disabled).toBe(false);
    expect(delRoadBtn.disabled).toBe(false);
    relevelBtn.click();
    delNodeBtn.click();
    delRoadBtn.click();
    expect(relevelRoadCalls).toBe(1);
    expect(deleteRoadNodeCalls).toBe(1);
    expect(deleteRoadCalls).toBe(1);
  });

  it('loads structured catalog.json into Texture Brush layers, categories, and PNG swatches', async () => {
    const structuredCatalog = {
      decorations: [
        {
          id: 'oak_tree',
          name: 'Oak Tree',
          path: '/world/assets/decorations/oak_tree.glb',
          category: 'Nature',
        },
      ],
      terrain_textures: [
        {
          layer: 0,
          id: 'grass',
          name: 'Meadow Grass',
          category: 'Grass',
          albedo_path: '/world/assets/terrain/grass.png',
          height_path: '/world/assets/terrain/grass_height.png',
        },
        {
          layer: 7,
          id: 'cobblestone',
          name: 'Town Cobble',
          category: 'Urban',
          albedo_path: '/world/assets/terrain/cobblestone.png',
          height_path: '/world/assets/terrain/cobblestone_height.png',
        },
        {
          layer: 9,
          id: 'snow',
          name: 'Fresh Snow',
          category: 'Winter',
          albedo_path: '/world/assets/terrain/snow.png',
          height_path: null,
        },
      ],
      road_textures: [
        {
          layer: 0,
          id: 'dirt_path',
          name: 'Forest Trail',
          edge: 'Soft',
          path: '/world/assets/roads/dirt_path.png',
        },
        {
          layer: 5,
          id: 'marble_way',
          name: 'Marble Way',
          edge: 'Curbed',
          path: '/world/assets/roads/marble_way.png',
        },
      ],
    };

    globalThis.fetch = (async () =>
      new Response(JSON.stringify(structuredCatalog), {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      })) as typeof fetch;

    await component.state.loadCatalog();
    statusSig.set(baseStatus('texture'));
    fixture.detectChanges();

    const root = fixture.nativeElement as HTMLElement;
    expect(component.state.catalog().map((d) => d.id)).toEqual(['oak_tree']);

    const pills = Array.from(root.querySelectorAll('.pill-btn')).map((p) => p.textContent?.trim());
    expect(pills).toEqual(['All', 'Grass', 'Urban', 'Winter']);

    const matNames = Array.from(root.querySelectorAll('.material-btn .material-main span:last-child')).map(
      (s) => s.textContent?.trim(),
    );
    expect(matNames).toEqual(['Meadow Grass', 'Town Cobble', 'Fresh Snow']);

    const swatches = Array.from(root.querySelectorAll('.material-btn .swatch')) as HTMLElement[];
    expect(swatches[0].style.backgroundImage).toContain('/world/assets/terrain/grass.png');
    expect(swatches[1].style.backgroundImage).toContain('/world/assets/terrain/cobblestone.png');
    expect(swatches[2].style.backgroundImage).toContain('/world/assets/terrain/snow.png');

    // Shortcut '3' selects the 3rd catalog entry (layer 9: Fresh Snow)
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    expect(textureCalls.at(-1)).toBe(9);

    // Filtering by Urban keeps only Town Cobble (layer 7)
    (root.querySelectorAll('.pill-btn')[2] as HTMLButtonElement).click();
    fixture.detectChanges();
    const filteredBtns = root.querySelectorAll('.material-btn');
    expect(filteredBtns.length).toBe(1);
    (filteredBtns[0] as HTMLButtonElement).click();
    expect(textureCalls.at(-1)).toBe(7);

    // Switching to Roads tool displays catalog road_textures with PNG swatches and custom layer IDs
    statusSig.set(baseStatus('road'));
    fixture.detectChanges();
    const roadBtns = Array.from(root.querySelectorAll('.road-mat-btn')) as HTMLButtonElement[];
    expect(roadBtns.length).toBe(2);
    expect(
      roadBtns.map((b) => b.querySelector('.road-mat-main span:last-child')?.textContent?.trim()),
    ).toEqual(['Forest Trail', 'Marble Way']);
    expect(roadBtns.map((b) => b.querySelector('.mat-edge')?.textContent?.trim())).toEqual([
      'Soft',
      'Curbed',
    ]);
    const roadSwatches = Array.from(root.querySelectorAll('.road-mat-btn .swatch')) as HTMLElement[];
    expect(roadSwatches[0].style.backgroundImage).toContain('/world/assets/roads/dirt_path.png');
    expect(roadSwatches[1].style.backgroundImage).toContain('/world/assets/roads/marble_way.png');

    roadBtns[1].click();
    expect(roadMaterialCalls.at(-1)).toBe(5);
  });

  it('dispatches Walkability tool shortcuts and room blockout actions', () => {
    const root = fixture.nativeElement as HTMLElement;
    const walkBtn = Array.from(
      root.querySelectorAll('app-editor-toolbar .group:nth-of-type(3) button'),
    ).find((b) => b.textContent?.trim() === 'Walkability') as HTMLButtonElement;
    walkBtn.click();
    fixture.detectChanges();

    expect(statusSig()?.tool).toBe('walkability');
    expect(root.querySelector('app-walkability-tool')).not.toBeNull();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '1' }));
    expect(walkabilityModeCalls).toEqual(['walkable', 'block']);

    (root.querySelector('.btn-bake') as HTMLButtonElement).click();
    expect(bakeWalkCalls).toBe(1);

    const resetBtn = Array.from(root.querySelectorAll('app-walkability-tool button')).find((b) =>
      b.textContent?.includes('Reset to Terrain Slope'),
    ) as HTMLButtonElement;
    resetBtn.click();
    expect(resetWalkCalls).toBe(1);
  });

  it('imports unique .glb files directly into the active area without polluting global catalog', async () => {
    component.state.selectedAreaId.set('cryo_prison');
    const uploadedUrls: string[] = [];

    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (init?.method === 'PUT') {
        uploadedUrls.push(url);
        return new Response(
          JSON.stringify({
            id: 'local:hero_cryo_pod',
            name: 'Hero Cryo Pod',
            path: '/world/areas/cryo_prison/assets/hero_cryo_pod.glb',
            category: 'Area Unique',
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      }
      if (url.includes('/api/editor/areas/cryo_prison/assets')) {
        return new Response(
          JSON.stringify([
            {
              id: 'local:hero_cryo_pod',
              name: 'Hero Cryo Pod',
              path: '/world/areas/cryo_prison/assets/hero_cryo_pod.glb',
              category: 'Area Unique',
            },
          ]),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      }
      return new Response('[]', { status: 200, headers: { 'Content-Type': 'application/json' } });
    }) as typeof fetch;

    const fakeGlb = new File([new Uint8Array([0x67, 0x6c, 0x54, 0x46, 2, 0, 0, 0, 12, 0, 0, 0])], 'Hero Cryo Pod.glb');
    await component.state.importUniqueGlbFiles([fakeGlb]);
    fixture.detectChanges();

    expect(uploadedUrls).toEqual(['/api/editor/areas/cryo_prison/assets/hero_cryo_pod']);
    expect(importedLocalGlbs).toEqual([{ slug: 'hero_cryo_pod', byteLen: 12, spawn: true }]);
    expect(component.state.catalog()).toEqual([]);
    expect(component.state.areaAssets().map((a) => a.id)).toEqual(['local:hero_cryo_pod']);

    const root = fixture.nativeElement as HTMLElement;
    expect(root.querySelector('.area-unique-drawer')).not.toBeNull();
  });

  it('switches top-level modes between World, Characters, and Entities and gates World hotkeys', () => {
    const root = fixture.nativeElement as HTMLElement;
    const modeBtns = Array.from(root.querySelectorAll('.mode-btn')) as HTMLButtonElement[];
    expect(modeBtns.map((b) => b.textContent?.trim())).toEqual(['World', 'Characters', 'Entities']);

    // Switch to Characters mode
    modeBtns[1].click();
    fixture.detectChanges();
    expect(modeCalls.at(-1)).toBe('characters');
    expect(root.querySelector('app-characters-editor')).not.toBeNull();
    expect(root.querySelector('aside.toolbox')).toBeNull();

    // World 1-4 hotkeys must be ignored while in Characters mode
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '3' }));
    expect(decorationCalls).toEqual([]);

    // Switch to Entities mode
    modeBtns[2].click();
    fixture.detectChanges();
    expect(modeCalls.at(-1)).toBe('entities');
    expect(root.querySelector('app-entities-placeholder')).not.toBeNull();
    expect(root.querySelector('app-characters-editor')).toBeNull();

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '4' }));
    expect(decorationCalls).toEqual([]);

    // Switch back to World mode and verify hotkeys work again
    modeBtns[0].click();
    fixture.detectChanges();
    expect(root.querySelector('aside.toolbox')).not.toBeNull();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '2' }));
    expect(decorationCalls).toEqual(['move']);
  });

  it('manages character presets, equips parts, applies/resets palette tints, and saves via PUT', async () => {
    await component.charState.initData();
    statusSig.update((s) => (s ? { ...s, editorMode: 'characters' } : s));
    fixture.detectChanges();

    const root = fixture.nativeElement as HTMLElement;
    expect(component.charState.activeCharacter().id).toBe('cryo_prisoner');

    // Switch preset to fire_raider
    component.charState.selectPreset('fire_raider');
    fixture.detectChanges();
    expect(component.charState.activeCharacter().id).toBe('fire_raider');
    expect(component.charState.activeCharacter().skinned_parts['head']).toBe('head_aug_visor');

    // Swap torso and socket part
    component.charState.setSkinnedPart('torso', 'torso_cryo_jumpsuit');
    component.charState.setSocketPart('socket_head', 'hair_short_crop');
    fixture.detectChanges();
    expect(activeCharactersPushed.at(-1)?.skinned_parts['torso']).toBe('torso_cryo_jumpsuit');
    expect(activeCharactersPushed.at(-1)?.socket_parts['socket_head']).toBe('hair_short_crop');
    expect(component.charState.saveState()).toBe('Unsaved');

    // Set and clear palette color
    component.charState.setPaletteColor('hair_color', '#ff8040');
    expect(component.charState.activeCharacter().palette.hair_color).toEqual([255, 128, 64]);
    component.charState.clearPaletteColor('hair_color');
    expect(component.charState.activeCharacter().palette.hair_color).toBeUndefined();

    // Save preset via PUT /api/editor/characters/presets/fire_raider
    const putCalls: { url: string; method: string; body: string }[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      const method = init?.method ?? 'GET';
      const bodyStr = typeof init?.body === 'string' ? init.body : '';
      putCalls.push({ url, method, body: bodyStr });
      return new Response(bodyStr, {
        status: 200,
        headers: { 'Content-Type': 'application/json' },
      });
    }) as typeof fetch;

    const saveBtn = Array.from(root.querySelectorAll('app-editor-toolbar button')).find(
      (b) => b.textContent?.trim() === 'Save',
    ) as HTMLButtonElement;
    saveBtn.click();
    await new Promise((resolve) => setTimeout(resolve, 0));
    fixture.detectChanges();

    expect(putCalls.length).toBe(1);
    expect(putCalls[0].method).toBe('PUT');
    expect(putCalls[0].url).toBe('/api/editor/characters/presets/fire_raider');
    expect(component.charState.saveState()).toBe('Saved');
  });

  it('dispatches character animation transport controls and bone debug toggle', async () => {
    await component.charState.initData();
    statusSig.update((s) => (s ? { ...s, editorMode: 'characters' } : s));
    fixture.detectChanges();

    const root = fixture.nativeElement as HTMLElement;
    const transportBtns = Array.from(
      root.querySelectorAll('.anim-transport button'),
    ) as HTMLButtonElement[];

    transportBtns.find((b) => b.textContent?.trim() === 'Walk')?.click();
    transportBtns.find((b) => b.textContent?.trim() === 'Bind Pose')?.click();
    expect(animClipCalls).toEqual(['Walk', '__bind__']);

    transportBtns.find((b) => b.textContent?.trim() === 'Idle')?.click();
    fixture.detectChanges();
    const playBtn = root.querySelector('.play-btn') as HTMLButtonElement;
    playBtn.click();
    expect(animPlayingCalls).toEqual([false]);

    transportBtns.find((b) => b.textContent?.trim() === '0.5x')?.click();
    expect(animSpeedCalls).toEqual([0.5]);

    const debugBtn = Array.from(root.querySelectorAll('app-editor-toolbar button')).find(
      (b) => b.textContent?.trim() === 'Bones & Sockets',
    ) as HTMLButtonElement;
    debugBtn.click();
    expect(debugSkeletonCalls).toEqual([true]);
  });

  it('edits socket offsets and imports a character .glb part in Parts & Rig mode', async () => {
    await component.charState.initData();
    statusSig.update((s) => (s ? { ...s, editorMode: 'characters' } : s));
    fixture.detectChanges();

    const root = fixture.nativeElement as HTMLElement;
    const rigTabBtn = Array.from(root.querySelectorAll('app-editor-toolbar button')).find(
      (b) => b.textContent?.trim() === 'Parts & Rig',
    ) as HTMLButtonElement;
    rigTabBtn.click();
    fixture.detectChanges();

    expect(root.querySelector('select[name="selectedSocket"]')).not.toBeNull();
    component.charState.selectSocket('socket_hand_r');
    component.charState.updateSocket('socket_hand_r', { offset: [0.02, -0.05, 0.08] });
    fixture.detectChanges();

    const updatedHandSock = catalogsPushed
      .at(-1)
      ?.skeletons[0]?.sockets.find((s) => s.id === 'socket_hand_r');
    expect(updatedHandSock?.offset).toEqual([0.02, -0.05, 0.08]);

    // Import a new .glb socket part
    const uploadRequests: string[] = [];
    globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (init?.method === 'PUT') {
        uploadRequests.push(url);
        return new Response(
          JSON.stringify({
            slug: 'plasma_pistol',
            name: 'Plasma Pistol',
            path: '/world/assets/characters/plasma_pistol.glb',
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        );
      }
      return new Response('{}', { status: 200 });
    }) as typeof fetch;

    const fakeGlb = new File(
      [new Uint8Array([0x67, 0x6c, 0x54, 0x46, 2, 0, 0, 0, 12, 0, 0, 0])],
      'Plasma Pistol.glb',
    );
    await component.charState.importCharacterGlbFiles([fakeGlb], {
      kind: 'socket',
      socketId: 'socket_hand_r',
      category: 'Weapons',
    });
    fixture.detectChanges();

    expect(uploadRequests).toEqual(['/api/editor/characters/assets/plasma_pistol']);
    expect(loadedCharGlbs).toEqual([
      { path: '/world/assets/characters/plasma_pistol.glb', byteLen: 12 },
    ]);
    expect(component.charState.activeCharacter().socket_parts['socket_hand_r']).toBe(
      'plasma_pistol',
    );
    expect(
      component.charState.catalog().parts.some((p) => p.id === 'plasma_pistol'),
    ).toBe(true);
  });
});

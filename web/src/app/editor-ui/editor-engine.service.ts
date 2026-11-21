import { effect, inject, Injectable, OnDestroy, signal } from '@angular/core';
import { EngineService } from '../engine.service';
import { EditorEngineHandle, EngineHandle } from '../wasm/client_wasm';

export type EditorMode = 'world' | 'characters' | 'entities';

export type AreaKind =
  | 'Outside'
  | { Inside: 'Generated' | 'Ephemeral' | 'Permanent' };

export interface SkinnedSlotDef {
  id: string;
  name: string;
}

export interface SocketDef {
  id: string;
  name: string;
  parent_bone: string;
  offset: [number, number, number];
  rotation_deg: [number, number, number];
  scale: number;
}

export interface SkeletonDef {
  id: string;
  name: string;
  rig_path: string;
  render_base_mesh: boolean;
  skinned_slots: SkinnedSlotDef[];
  sockets: SocketDef[];
  animation_paths: string[];
}

export type PartAttachment =
  | { kind: 'skinned'; slot: string }
  | {
      kind: 'socket';
      socket_id: string;
      offset: [number, number, number];
      rotation_deg: [number, number, number];
      scale: number;
    };

export interface CharacterPartDef {
  id: string;
  name: string;
  skeleton_id: string;
  attachment: PartAttachment;
  path: string;
  category: string;
}

export interface CharacterCatalog {
  skeletons: SkeletonDef[];
  parts: CharacterPartDef[];
}

export interface CharacterPalette {
  skin_color?: [number, number, number] | null;
  hair_color?: [number, number, number] | null;
  eye_color?: [number, number, number] | null;
  primary_color?: [number, number, number] | null;
  secondary_color?: [number, number, number] | null;
  accent_color?: [number, number, number] | null;
}

export interface CharacterDefinition {
  id: string;
  name: string;
  skeleton_id: string;
  show_base_mesh: boolean;
  skinned_parts: Record<string, string>;
  socket_parts: Record<string, string>;
  palette: CharacterPalette;
}

export interface CharacterStudioStatus {
  activePresetId: string;
  activeSkeletonId: string;
  availableClips: string[];
  activeClip: string | null;
  playing: boolean;
  animTime: number;
  clipDuration: number;
  playbackSpeed: number;
  showSkeletonDebug: boolean;
  turntable: boolean;
  selectedSocketId: string | null;
  boneNames: string[];
  saveState: string;
}

export interface AreaMetadata {
  id: string;
  name: string;
  kind: AreaKind;
  dimensions: [number, number] | null;
}

export interface AssetEntry {
  id: string;
  name: string;
  path: string;
  category: string;
}

export interface TerrainTextureEntry {
  layer: number;
  id: string;
  name: string;
  category: string;
  albedo_path: string;
  height_path?: string | null;
}

export interface RoadTextureEntry {
  layer: number;
  id: string;
  name: string;
  edge: string;
  path: string;
}

export interface AssetCatalog {
  decorations?: AssetEntry[];
  terrain_textures?: TerrainTextureEntry[];
  road_textures?: RoadTextureEntry[];
}

export interface TextureLayerOption {
  index: number;
  label: string;
  category: string;
  swatch: string;
  albedoPath: string | null;
  shortcut: string;
}

export interface RoadMaterialOption {
  index: number;
  label: string;
  edge: string;
  swatch: string;
  path: string | null;
}

export const DEFAULT_ROAD_MATERIALS: readonly RoadMaterialOption[] = [
  { index: 0, label: 'Dirt Path', edge: 'Soft', swatch: '#8b6642', path: null },
  { index: 1, label: 'Cobblestone', edge: 'Curbed', swatch: '#767980', path: null },
  { index: 2, label: 'Brick', edge: 'Curbed', swatch: '#a14d3a', path: null },
  { index: 3, label: 'Asphalt', edge: 'Curbed', swatch: '#373a40', path: null },
  { index: 4, label: 'Flagstone', edge: 'Border', swatch: '#7f847c', path: null },
];

export const DEFAULT_TEXTURE_LAYERS: readonly TextureLayerOption[] = [
  { index: 0, label: 'Grass', category: 'Grass', swatch: '#4a8f3d', albedoPath: null, shortcut: '1' },
  { index: 4, label: 'Light Grass', category: 'Grass', swatch: '#76b546', albedoPath: null, shortcut: '2' },
  { index: 5, label: 'Dark Grass', category: 'Grass', swatch: '#2a5f32', albedoPath: null, shortcut: '3' },
  { index: 6, label: 'Patchy Grass', category: 'Grass', swatch: '#6d7b3a', albedoPath: null, shortcut: '4' },
  { index: 1, label: 'Dirt', category: 'Earth', swatch: '#8b623f', albedoPath: null, shortcut: '5' },
  { index: 3, label: 'Sand', category: 'Earth', swatch: '#d7be7a', albedoPath: null, shortcut: '6' },
  { index: 2, label: 'Rock', category: 'Stone', swatch: '#787e88', albedoPath: null, shortcut: '7' },
  { index: 7, label: 'Cobblestone', category: 'Stone', swatch: '#72757b', albedoPath: null, shortcut: '8' },
  { index: 8, label: 'Brick', category: 'Stone', swatch: '#9c4c3a', albedoPath: null, shortcut: '9' },
];

export interface SelectedPropInfo {
  id: number;
  assetId: string;
  position: [number, number, number];
  rotationDeg: [number, number, number];
  scale: number;
  castShadow: boolean;
}

export interface SelectedRoadInfo {
  id: number;
  style: 'curve' | 'straight';
  material: number;
  width: number;
  shoulder: number;
  pointCount: number;
  selectedNode: number | null;
}

export type TriggerActivation = 'click' | 'walk_into' | 'both';

export interface TriggerConfig {
  size: [number, number, number];
  offset: [number, number, number];
  yaw_deg: number;
  activation: TriggerActivation;
  interaction_range: number;
}

export type VisualConfig =
  | { character: { preset_id: string } }
  | { prop: { asset_id: string; scale: [number, number, number]; cast_shadow: boolean } };

export interface MovementConfig {
  speed: number;
}

export interface ScriptConfig {
  source: string;
  compiled_js: string;
}

export type PropertyValue =
  | { Bool: boolean }
  | { Int: number }
  | { Float: number }
  | { String: string };

export interface EntityTemplate {
  id: string;
  name: string;
  properties: Record<string, PropertyValue>;
  visual: VisualConfig | null;
  movement: MovementConfig | null;
  trigger: TriggerConfig | null;
  script: ScriptConfig | null;
}

export type ComponentOverride<T> = 'inherit' | { set: T } | 'remove';

export interface EntityOverrides {
  name: string | null;
  properties: Record<string, PropertyValue>;
  removed_properties: string[];
  visual: ComponentOverride<VisualConfig>;
  movement: ComponentOverride<MovementConfig>;
  trigger: ComponentOverride<TriggerConfig>;
  script: ComponentOverride<ScriptConfig>;
}

export interface PlacedEntity {
  id: number;
  template_id: string;
  position: [number, number, number];
  rotation_y: number;
  overrides: EntityOverrides;
}

export interface EditorStatus {
  editorMode: EditorMode;
  areaId: string;
  areaName: string;
  isOutside: boolean;
  currentTile: [number, number];
  dimensions: [number, number] | null;
  cursorPos: [number, number, number] | null;
  dirtyCount: number;
  saveState: string;
  tool: 'heightmap' | 'texture' | 'road' | 'decoration' | 'entities' | 'walkability';
  heightMode: 'raise' | 'lower' | 'smooth' | 'flatten';
  textureLayer: number;
  roadSubmode: 'place' | 'edit';
  roadStyle: 'curve' | 'straight';
  roadMaterial: number;
  roadWidth: number;
  roadShoulder: number;
  roadDraftPoints: number;
  selectedRoad: SelectedRoadInfo | null;
  decorationSubmode: 'place' | 'move' | 'free_move' | 'rotate';
  selectedProp: SelectedPropInfo | null;
  previewRotationDeg: [number, number, number];
  previewScale: number;
  previewCastShadow: boolean;
  entitySubmode: 'place' | 'move' | 'free_move' | 'rotate';
  activeEntityTemplateId: string;
  entityPreviewYawDeg: number;
  selectedEntity: PlacedEntity | null;
  entityStudioTurntable: boolean;
  walkabilityMode: 'block' | 'walkable';
  characterStudio: CharacterStudioStatus;
}

export function formatAreaKind(meta: AreaMetadata): string {
  if (meta.kind === 'Outside') {
    return 'Outside';
  }
  const sub = meta.kind.Inside;
  if (meta.dimensions) {
    return `Inside · ${sub} ${meta.dimensions[0]}×${meta.dimensions[1]}m`;
  }
  return `Inside · ${sub}`;
}

@Injectable({ providedIn: 'root' })
export class EditorEngineService implements OnDestroy {
  private readonly core = inject(EngineService);

  private handle: EditorEngineHandle | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private pendingMode: EditorMode | null = null;
  private pendingArea: AreaMetadata | null = null;
  private pendingCharacterCatalog: CharacterCatalog | null = null;
  private pendingActiveCharacter: CharacterDefinition | null = null;
  private pendingEntityTemplates: EntityTemplate[] | null = null;
  private pendingCharacterPresets: CharacterDefinition[] | null = null;
  private pendingActiveEntityTemplate: EntityTemplate | null | undefined = undefined;
  private pendingPlacementEntityTemplate: string | null = null;

  readonly ready = this.core.ready;
  readonly initError = this.core.initError;
  readonly editorEnabled = signal(false);
  readonly status = signal<EditorStatus | null>(null);

  constructor() {
    effect(() => {
      if (!this.core.ready()) {
        return;
      }
      const isEd = EngineHandle.is_editor_enabled();
      this.editorEnabled.set(isEd);
      if (!isEd) {
        return;
      }
      const h = this.ensureHandle();
      if (h && this.pendingMode) {
        h.set_editor_mode(this.pendingMode);
        this.pendingMode = null;
      }
      if (h && this.pendingArea) {
        h.set_area(JSON.stringify(this.pendingArea));
        this.pendingArea = null;
      }
      if (h && this.pendingCharacterCatalog) {
        h.set_character_catalog_json(JSON.stringify(this.pendingCharacterCatalog));
        this.pendingCharacterCatalog = null;
      }
      if (h && this.pendingActiveCharacter) {
        h.set_active_character_json(JSON.stringify(this.pendingActiveCharacter));
        this.pendingActiveCharacter = null;
      }
      if (h && this.pendingCharacterPresets) {
        h.set_character_presets_json(JSON.stringify(this.pendingCharacterPresets));
        this.pendingCharacterPresets = null;
      }
      if (h && this.pendingEntityTemplates) {
        h.set_entity_templates_json(JSON.stringify(this.pendingEntityTemplates));
        this.pendingEntityTemplates = null;
      }
      if (h && this.pendingActiveEntityTemplate !== undefined) {
        h.set_active_entity_template_json(JSON.stringify(this.pendingActiveEntityTemplate));
        this.pendingActiveEntityTemplate = undefined;
      }
      if (h && this.pendingPlacementEntityTemplate !== null) {
        h.set_placement_entity_template(this.pendingPlacementEntityTemplate);
        this.pendingPlacementEntityTemplate = null;
      }
      this.refreshStatus();
      if (this.pollTimer === null) {
        this.pollTimer = setInterval(() => this.refreshStatus(), 100);
      }
    });
  }

  private ensureHandle(): EditorEngineHandle | null {
    if (this.handle) {
      return this.handle;
    }
    const raw = this.core.rawHandle;
    if (!raw || !EngineHandle.is_editor_enabled()) {
      return null;
    }
    this.editorEnabled.set(true);
    this.handle = new EditorEngineHandle(raw);
    return this.handle;
  }

  private refreshStatus(): void {
    const h = this.ensureHandle();
    if (!h) return;
    try {
      const parsed = JSON.parse(h.get_editor_status_json()) as EditorStatus;
      this.status.set(parsed);
    } catch {
      // ignore transient parse error
    }
  }

  setArea(meta: AreaMetadata): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingArea = meta;
      return;
    }
    h.set_area(JSON.stringify(meta));
    this.refreshStatus();
  }

  setTool(tool: 'heightmap' | 'texture' | 'road' | 'decoration' | 'entities' | 'walkability'): void {
    this.ensureHandle()?.set_editor_tool(tool);
    this.refreshStatus();
  }

  setWalkabilityMode(mode: 'block' | 'walkable'): void {
    this.ensureHandle()?.set_walkability_mode(mode);
    this.refreshStatus();
  }

  bakePropsToWalkability(): void {
    this.ensureHandle()?.bake_props_to_walkability();
    this.refreshStatus();
  }

  resetWalkabilityToSlope(): void {
    this.ensureHandle()?.reset_walkability_to_slope();
    this.refreshStatus();
  }

  importLocalGlb(slug: string, bytes: Uint8Array, spawnInScene = true): string | null {
    const h = this.ensureHandle();
    if (!h) {
      return null;
    }
    const assetId = h.import_local_glb(slug, bytes, spawnInScene);
    this.refreshStatus();
    return assetId;
  }

  setHeightMode(mode: 'raise' | 'lower' | 'smooth' | 'flatten'): void {
    this.ensureHandle()?.set_height_mode(mode);
    this.refreshStatus();
  }

  setBrushRadius(radius: number): void {
    this.ensureHandle()?.set_brush_radius(radius);
  }

  setBrushStrength(strength: number): void {
    this.ensureHandle()?.set_brush_strength(strength);
  }

  setTextureLayer(layer: number): void {
    this.ensureHandle()?.set_texture_layer(layer);
    this.refreshStatus();
  }

  setRoadSubmode(submode: 'place' | 'edit'): void {
    this.ensureHandle()?.set_road_submode(submode);
    this.refreshStatus();
  }

  setRoadStyle(style: 'curve' | 'straight'): void {
    this.ensureHandle()?.set_road_style(style);
    this.refreshStatus();
  }

  setRoadMaterial(material: number): void {
    this.ensureHandle()?.set_road_material(material);
    this.refreshStatus();
  }

  setRoadWidth(width: number): void {
    this.ensureHandle()?.set_road_width(width);
    this.refreshStatus();
  }

  setRoadShoulder(shoulder: number): void {
    this.ensureHandle()?.set_road_shoulder(shoulder);
    this.refreshStatus();
  }

  finishActiveRoad(): void {
    this.ensureHandle()?.finish_active_road();
    this.refreshStatus();
  }

  cancelActiveRoad(): void {
    this.ensureHandle()?.cancel_active_road();
    this.refreshStatus();
  }

  relevelSelectedRoad(): void {
    this.ensureHandle()?.relevel_selected_road();
    this.refreshStatus();
  }

  deleteSelectedRoadNode(): void {
    this.ensureHandle()?.delete_selected_road_node();
    this.refreshStatus();
  }

  deleteSelectedRoad(): void {
    this.ensureHandle()?.delete_selected_road();
    this.refreshStatus();
  }

  setDecorationSubmode(submode: 'place' | 'move' | 'free_move' | 'rotate'): void {
    this.ensureHandle()?.set_decoration_submode(submode);
    this.refreshStatus();
  }

  snapSelectedPropToTerrain(): void {
    this.ensureHandle()?.snap_selected_prop_to_terrain();
    this.refreshStatus();
  }

  setDecorationAsset(assetId: string): void {
    this.ensureHandle()?.set_decoration_asset(assetId);
  }

  setPropRotationAxisDeg(axis: 0 | 1 | 2, deg: number): void {
    this.ensureHandle()?.set_selected_prop_rotation_axis_deg(axis, deg);
    this.refreshStatus();
  }

  resetPropRotation(): void {
    this.ensureHandle()?.reset_selected_prop_rotation();
    this.refreshStatus();
  }

  setPropScale(scale: number): void {
    this.ensureHandle()?.set_selected_prop_scale(scale);
    this.refreshStatus();
  }

  setPropCastShadow(castShadow: boolean): void {
    this.ensureHandle()?.set_selected_prop_cast_shadow(castShadow);
    this.refreshStatus();
  }

  duplicateSelectedProp(): void {
    this.ensureHandle()?.duplicate_selected_prop();
    this.refreshStatus();
  }

  deleteSelectedProp(): void {
    this.ensureHandle()?.delete_selected_prop();
    this.refreshStatus();
  }

  saveNow(): void {
    this.ensureHandle()?.save_editor_changes();
    this.refreshStatus();
  }

  setEditorMode(mode: EditorMode): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingMode = mode;
      return;
    }
    h.set_editor_mode(mode);
    this.refreshStatus();
  }

  setCharacterCatalog(catalog: CharacterCatalog): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingCharacterCatalog = catalog;
      return;
    }
    h.set_character_catalog_json(JSON.stringify(catalog));
    this.refreshStatus();
  }

  setActiveCharacter(def: CharacterDefinition): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingActiveCharacter = def;
      return;
    }
    h.set_active_character_json(JSON.stringify(def));
    this.refreshStatus();
  }

  loadCharacterGlbBytes(path: string, bytes: Uint8Array): void {
    this.ensureHandle()?.load_character_glb_bytes(path, bytes);
    this.refreshStatus();
  }

  setCharacterAnimClip(clip: string): void {
    this.ensureHandle()?.set_character_anim_clip(clip);
    this.refreshStatus();
  }

  setCharacterAnimPlaying(playing: boolean): void {
    this.ensureHandle()?.set_character_anim_playing(playing);
    this.refreshStatus();
  }

  setCharacterAnimTime(timeSec: number): void {
    this.ensureHandle()?.set_character_anim_time(timeSec);
    this.refreshStatus();
  }

  setCharacterAnimSpeed(speed: number): void {
    this.ensureHandle()?.set_character_anim_speed(speed);
    this.refreshStatus();
  }

  setCharacterDebugSkeleton(show: boolean): void {
    this.ensureHandle()?.set_character_debug_skeleton(show);
    this.refreshStatus();
  }

  setCharacterTurntable(enabled: boolean): void {
    this.ensureHandle()?.set_character_turntable(enabled);
    this.refreshStatus();
  }

  setSelectedSocketId(socketId: string | null): void {
    this.ensureHandle()?.set_selected_socket_id(socketId ?? '');
    this.refreshStatus();
  }

  resetCharacterStudioCamera(): void {
    this.ensureHandle()?.reset_character_studio_camera();
  }

  setEntityTemplates(templates: EntityTemplate[]): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingEntityTemplates = templates;
      return;
    }
    h.set_entity_templates_json(JSON.stringify(templates));
    this.refreshStatus();
  }

  setCharacterPresets(presets: CharacterDefinition[]): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingCharacterPresets = presets;
      return;
    }
    h.set_character_presets_json(JSON.stringify(presets));
    this.refreshStatus();
  }

  setActiveEntityTemplate(template: EntityTemplate | null): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingActiveEntityTemplate = template;
      return;
    }
    h.set_active_entity_template_json(JSON.stringify(template));
    this.refreshStatus();
  }

  setEntityStudioTurntable(enabled: boolean): void {
    this.ensureHandle()?.set_entity_studio_turntable(enabled);
    this.refreshStatus();
  }

  setEntitySubmode(submode: 'place' | 'move' | 'free_move' | 'rotate'): void {
    this.ensureHandle()?.set_entity_submode(submode);
    this.refreshStatus();
  }

  setPlacementEntityTemplate(templateId: string): void {
    const h = this.ensureHandle();
    if (!h) {
      this.pendingPlacementEntityTemplate = templateId;
      return;
    }
    h.set_placement_entity_template(templateId);
    this.refreshStatus();
  }

  updateSelectedPlacedEntity(entity: PlacedEntity): void {
    this.ensureHandle()?.update_selected_placed_entity_json(JSON.stringify(entity));
    this.refreshStatus();
  }

  snapSelectedEntityToTerrain(): void {
    this.ensureHandle()?.snap_selected_entity_to_terrain();
    this.refreshStatus();
  }

  duplicateSelectedEntity(): void {
    this.ensureHandle()?.duplicate_selected_entity();
    this.refreshStatus();
  }

  deleteSelectedEntity(): void {
    this.ensureHandle()?.delete_selected_entity();
    this.refreshStatus();
  }

  ngOnDestroy(): void {
    if (this.pollTimer !== null) {
      clearInterval(this.pollTimer);
      this.pollTimer = null;
    }
    this.handle?.free();
    this.handle = null;
  }
}
